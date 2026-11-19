//! Extend the local hunter's command table at GU's binding point.
use core::{cell::UnsafeCell, ptr};
use mhgu_sdk::style::action::{
    self,
    command::{
        BufferedCommand, MAX_COMMAND_FRAME,
        native::{CAPACITY, Command, Entry, Resource, Table, buffer, table},
    },
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
    unsafe fn append(&mut self, actor: u32, stock: Table, rows: &[BufferedCommand]) {
        let first = stock.count as usize;
        let count = first + rows.len();
        if first != 0 {
            unsafe { ptr::copy_nonoverlapping(stock.entries, self.entries.as_mut_ptr(), first) };
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
        let Some(style) = crate::runtime::local_style(actor) else {
            return;
        };
        let Some(bind) = style.commands else {
            return;
        };
        // 2E9B7C only resets the native buffer for an enabled hunter.
        if ((actor + 5068) as *const u32).read() & 1 == 0 {
            return;
        }
        let Some(set) = bind(actor, id).as_ref() else {
            return;
        };
        let stock = table(actor)
            .read()
            .as_ref()
            .copied()
            .unwrap_or(Table::EMPTY);
        let rows = set.rows();
        if rows.is_empty()
            || rows.len() > CAPACITY.saturating_sub(stock.count as usize)
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
        (*BINDING.0.get()).append(actor, stock, rows);
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
        let frame = ((actor + 1280) as *const f32).read();
        if !(open as f32..close as f32).contains(&frame) {
            return 0;
        }
        let native = Command {
            condition: opcode as u16,
            ..*command
        };
        original!(actor, &native)
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, bind);
    forge::install_hook!(base, condition);
}
