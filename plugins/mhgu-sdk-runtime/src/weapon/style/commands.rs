//! Extend the local hunter's command table at GU's binding point.
use super::{Entry as StyleEntry, local_entry, routes};
use core::{cell::UnsafeCell, ptr};
use mhgu_sdk::weapon::style::{
    action::{
        self,
        command::{
            CommandSet, MAX_COMMAND_FRAME,
            native::{
                ATTACK_CHORD, CAPACITY, Command, Entry, Resource, Table, WEAPON_AFTER_COMBO,
                WEAPON_CONDITION, buffer, table,
            },
        },
    },
    input,
};

const BUFFERED_INPUT: u16 = 0x0fff;

struct Binding {
    table: Table,
    entries: [*const Entry; CAPACITY],
    resources: [Resource; CAPACITY],
    commands: [Entry; CAPACITY],
    owner: u32,
    first: usize,
}

struct LocalBinding(UnsafeCell<Binding>);
// GU binds and evaluates the local hunter's table on the game thread.
unsafe impl Sync for LocalBinding {}

static BINDING: LocalBinding = LocalBinding(UnsafeCell::new(Binding {
    table: Table::EMPTY,
    entries: [ptr::null(); CAPACITY],
    resources: [const { Resource::EMPTY }; CAPACITY],
    commands: [const { Entry::EMPTY }; CAPACITY],
    owner: 0,
    first: 0,
}));

impl Binding {
    unsafe fn bind(
        &mut self,
        actor: u32,
        stock: Table,
        style: &StyleEntry,
        set: Option<&CommandSet>,
    ) {
        let mut first = 0;
        for i in 0..stock.count as usize {
            let entry = unsafe { stock.entries.add(i).read() };
            let command =
                unsafe { entry.as_ref().and_then(|e| e.resource.as_ref()) }.map(|r| &r.command);
            if set.is_some_and(|set| !set.keeps_native(command.map(|c| (c.group, c.slot)))) {
                continue;
            }
            // GU may consume a buffered row before the state-change hook can
            // decline it. Remove fallback weapon branches at binding time.
            if style.kind.is_some()
                && let Some(command) = command
                && command.group & 0x7fff == action::GROUP
                && command.condition != action::Transition::HUNTER_ART.condition
                && !unsafe { routes::shared_weapon_slot(actor, command.slot) }
            {
                continue;
            }
            self.entries[first] = entry;
            first += 1;
        }
        let rows = set.map_or(&[][..], CommandSet::rows);
        let count = first + rows.len();
        if count > CAPACITY {
            return;
        }
        for (i, row) in rows.iter().enumerate() {
            let (opcode, input) = row.input.opcode_and_id();
            let pre_time = row.execute_from - row.accept_from;
            let open = if pre_time == 0 { row.execute_from } else { 0 };
            self.resources[i].command = Command {
                group: row.group,
                slot: row.slot,
                flags: 0,
                condition: BUFFERED_INPUT,
                parameters: [
                    input.into(),
                    row.accept_until as u32,
                    opcode.into(),
                    open as u32,
                ],
            };
            self.commands[i] = Entry::new(
                &self.resources[i],
                pre_time,
                row.execute_from,
                MAX_COMMAND_FRAME,
            );
            self.entries[first + i] = &self.commands[i];
        }
        self.owner = actor;
        self.first = first;
        self.table = Table {
            count: count as u32,
            capacity: count as u32,
            entries: self.entries.as_ptr(),
            ..stock
        };
        unsafe { table(actor).write(&self.table) };
    }
}

#[forge::hook(offset = 0x2e9b7c)]
unsafe fn bind(actor: u32, id: u32, origin: u32) {
    original!(actor, id, origin);
    if matches!(id, 0 | 9999) {
        return;
    }
    unsafe {
        let Some(entry) = local_entry(actor) else {
            return;
        };
        // 2E9B7C only resets the native buffer for an enabled hunter.
        if ((actor + 5068) as *const u32).read() & 1 == 0 {
            return;
        }
        let style = entry.style;
        let set = style.commands.and_then(|bind| bind(actor, id).as_ref());
        if set.is_none() && entry.kind.is_none() {
            return;
        }
        let stock = table(actor)
            .read()
            .as_ref()
            .copied()
            .unwrap_or(Table::EMPTY);
        let rows = set.map_or(&[][..], CommandSet::rows);
        if rows.len() > CAPACITY
            || stock.count as usize > CAPACITY
            || rows.iter().any(|row| {
                let (_, input) = row.input.opcode_and_id();
                let valid_action = row.group != action::GROUP
                    || action::custom_id(row.group, row.slot)
                        .is_some_and(|id| style.actions.contains(id));
                !valid_action
                    || input >= 128
                    || row.accept_from < 0
                    || row.execute_from < row.accept_from
                    || row.execute_from >= MAX_COMMAND_FRAME
                    || row.accept_until <= row.accept_from
                    || row.accept_until > MAX_COMMAND_FRAME
            })
        {
            return;
        }
        (*BINDING.0.get()).bind(actor, stock, entry, set);
    }
}

#[forge::hook(offset = 0x2ecefc)]
unsafe fn condition(actor: u32, command: *const Command) -> u32 {
    let command = unsafe { &*command };
    if command.condition != BUFFERED_INPUT {
        return original!(actor, ptr::from_ref(command));
    }
    unsafe {
        let binding = &*BINDING.0.get();
        if binding.owner != actor || !ptr::eq(table(actor).read(), &binding.table) {
            return 0;
        }
        let latched = &*buffer(actor);
        if latched[binding.first..binding.table.count as usize]
            .iter()
            .any(|&value| value != 0)
        {
            // GU checks the saved byte even when the condition returns false.
            return 0;
        }
        // GU's timing test can include the upcoming frame. An immediate
        // command still opens and closes on the current animation frame.
        let [_, close, opcode, open] = command.parameters;
        let sample = action::MotionFrame::sample(actor);
        let close = close as f32;
        if !(open as f32..close).contains(&sample.frame) {
            return 0;
        }
        let opcode = match opcode as u16 {
            // Keep the last valid frame available for a standalone press.
            WEAPON_AFTER_COMBO if sample.next < close && input::attack_combo_pending(actor) => {
                return 0;
            }
            ATTACK_CHORD
                if input::attack_chord_held(actor) || input::attack_chord_with_shoulder(actor) =>
            {
                return 1;
            }
            WEAPON_AFTER_COMBO | ATTACK_CHORD => WEAPON_CONDITION,
            opcode => opcode,
        };
        let native = Command {
            condition: opcode,
            ..*command
        };
        original!(actor, &native)
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, bind);
    forge::install_hook!(base, condition);
}
