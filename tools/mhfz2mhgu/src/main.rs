mod charge;
mod emc;
mod hunter;
mod hunter_hud;
mod hunter_fx;
mod model;
mod motion;
mod package;
mod sound;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mhf_resource::{ai, fmod::Fmod, motion::MotionArchive, pac, rig::Rig};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(version, about = "Convert Frontier assets to MHGU (Switch) resources")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Monster {
    /// Frontier emmodel PAC
    pac: PathBuf,
    /// PAC member that contains the FMOD and FSKL pair
    #[arg(long, default_value_t = 0)]
    model_member: usize,
}
impl Monster {
    fn open(&self) -> Result<(Vec<u8>, Vec<u8>, Rig)> {
        let pac = pac::decode(read(&self.pac)?)?;
        let pair = pac::member(&pac, self.model_member)?;
        let rig = Rig::parse(&pac::member(&pair, 1)?).context("read FSKL")?;
        Ok((pac, pair, rig))
    }
}

#[derive(Subcommand)]
enum Command {
    /// Extract the original great sword emitters, their GU meshes and effect atlases.
    GreatSwordEffects {
        dat: PathBuf,
        /// GU quest/w00.arc, which supplies the native particle material
        #[arg(long)]
        gu_weapon: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert the original Frontier great sword's private sound resources for GU.
    GreatSwordSound {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Join Frontier great sword motion layers and map them to GU hunter joints
    GreatSwordMotions {
        /// Frontier dat directory (motion/w00.mot and parts/<sex>00/<sex>_editpl.bin)
        dat: PathBuf,
        #[arg(long)]
        female: bool,
        /// Output independent GU motion ARC; never replaces a native style
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Extract the FZ extreme great sword action program with file-relative offsets
    GreatSwordActions {
        /// Frontier dat directory (mhfdat.bin and mhfsdt.bin)
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert the paired Frontier dual-blade motion directories.
    DualBladesMotions {
        dat: PathBuf,
        #[arg(long)]
        female: bool,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Extract Frontier dual-blade moves, contacts and weapon parameters.
    DualBladesActions {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert the Frontier dual-blade motion sound timeline and request banks.
    DualBladesSound {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build the private flame gauge and blade-polish sprite atlas for GU.
    DualBladesHud {
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert the Frontier dual-blade evade particle bank.
    DualBladesEffects {
        dat: PathBuf,
        #[arg(long)]
        gu_weapon: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Show the source model, skeleton and motion groups before converting
    Inspect {
        #[command(flatten)]
        monster: Monster,
        #[arg(long, default_value_t = 2)]
        motion_member: usize,
    },
    /// Write a skinned MOD 0xD4 mesh with its skeleton and inverse bind matrices
    Model {
        #[command(flatten)]
        monster: Monster,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Write an LMT v67 animation list for one source MOT group
    Motion {
        #[command(flatten)]
        monster: Monster,
        #[arg(long, default_value_t = 2)]
        motion_member: usize,
        #[arg(long)]
        group: usize,
        /// Only export this slot, retaining its original index in the list
        #[arg(long)]
        slot: Option<usize>,
        /// FSKL root-table ordinal, for ambiguous bindings
        #[arg(long)]
        root: Option<usize>,
        /// FSKL motion tag, for ambiguous bindings
        #[arg(long)]
        tag: Option<u16>,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build a visual resource ARC (model, materials, textures and a motion group)
    Package {
        #[command(flatten)]
        monster: Monster,
        /// Native MRL or monster ARC with static opaque and cutout materials (em030_00.arc)
        #[arg(long)]
        material_template: PathBuf,
        /// Target resource stem; does not register a monster ID
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = 1)]
        texture_member: usize,
        #[arg(long, default_value_t = 2)]
        motion_member: usize,
        #[arg(long, default_value_t = 0)]
        group: usize,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Export the ordinary Espinas icon using Frontier's species-to-icon table
    HuntIcon {
        #[arg(long)]
        data: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build one complete Espinas quest overlay; rejects missing source action handlers
    Hunt {
        pac: PathBuf,
        #[arg(long)]
        donor: PathBuf,
        #[arg(long)]
        material_template: PathBuf,
        #[arg(long, default_value = "mhfz/eboot.bin.elf")]
        vita_elf: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build an experimental single-charge overlay for the ordinary GU Rathian quest slot
    ChargeTest {
        /// Standard-detail Frontier em080.pac
        pac: PathBuf,
        /// Source Vita executable containing Espinas body volumes
        #[arg(long, default_value = "mhfz/eboot.bin.elf")]
        vita_elf: PathBuf,
        /// Native em001_00.arc to retain quest resources and action handlers
        #[arg(long)]
        donor: PathBuf,
        #[arg(long)]
        material_template: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build a fixed-input replay of a supported original ground battle choice
    CombatReplay {
        pac: PathBuf,
        #[arg(long)]
        donor: PathBuf,
        #[arg(long)]
        material_template: PathBuf,
        #[arg(long, default_value = "mhfz/eboot.bin.elf")]
        vita_elf: PathBuf,
        #[arg(long)]
        distance: f32,
        #[arg(long, default_value_t = 0)]
        angle_units: u16,
        #[arg(long)]
        enraged: bool,
        #[arg(long)]
        random: u16,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Bake every Espinas base clip with its available tagged poses onto the Rathian graft
    CombatMotions {
        pac: PathBuf,
        #[arg(long)]
        donor: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Write the ordinary Espinas state 0:3 battle choices as a CSV of Frontier requests
    CombatChoices {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Follow the Vita ELF's monster script tables; without -o, list reachable groups and slots
    Emc {
        input: PathBuf,
        /// Frontier monster ID (80 = Espinas)
        #[arg(long)]
        monster: u8,
        /// Native selector configuration
        #[arg(long, default_value_t = 0)]
        bank: u8,
        /// Root of the scan in FZ GROUP:SLOT notation
        #[arg(long, default_value = "0:0")]
        entry: ai::Entry,
        /// Export one isolated action example, without the surrounding AI conditions
        #[arg(long, requires = "output")]
        action_sample: bool,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("read {}", path.display()))
}

fn save(path: &Path, bytes: Vec<u8>) -> Result<()> {
    fs::write(path, &bytes).with_context(|| format!("write {}", path.display()))?;
    println!("{} ({} bytes)", path.display(), bytes.len());
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::GreatSwordEffects {
            dat,
            output,
            gu_weapon,
        } => hunter_fx::extract(&dat, &output, &gu_weapon),
        Command::GreatSwordSound { dat, output } => {
            let (assets, timeline) = sound::great_sword(&dat)?;
            save(&output.with_extension("fgse"), timeline)?;
            save(&output, mhgu_resource::arc::encode(&assets)?)
        }
        Command::GreatSwordActions { dat, output } => save(&output, hunter::actions(&dat)?),
        Command::GreatSwordMotions {
            dat,
            female,
            output,
        } => save(&output, hunter::great_sword(&dat, female)?),
        Command::DualBladesMotions {
            dat,
            female,
            output,
        } => save(&output, hunter::dual_blades(&dat, female)?),
        Command::DualBladesActions { dat, output } => {
            save(&output, hunter::dual_blades_actions(&dat)?)
        }
        Command::DualBladesSound { dat, output } => {
            let (assets, timeline) = sound::dual_blades(&dat)?;
            save(&output.with_extension("fdse"), timeline)?;
            save(&output, mhgu_resource::arc::encode(&assets)?)
        }
        Command::DualBladesHud { output } => save(&output, hunter_hud::dual_blades()?),
        Command::DualBladesEffects {
            dat,
            output,
            gu_weapon,
        } => hunter_fx::dual_blades(&dat, &output, &gu_weapon),
        Command::Model { monster, output } => {
            let (_, pair, rig) = monster.open()?;
            save(&output, model::convert(&pac::member(&pair, 0)?, &rig)?)
        }
        Command::Motion {
            monster,
            motion_member,
            group,
            slot,
            root,
            tag,
            output,
        } => {
            let (pac, _, rig) = monster.open()?;
            save(
                &output,
                motion::convert(
                    &pac::member(&pac, motion_member)?,
                    &rig,
                    group,
                    slot,
                    tag,
                    root,
                )?,
            )
        }
        Command::Inspect {
            monster,
            motion_member,
        } => {
            let (pac, pair, rig) = monster.open()?;
            let model = pac::member(&pair, 0)?;
            let f = Fmod::parse(&model)?;
            println!(
                "{} FMOD objects, {} materials, {} FSKL bones, roots {:?}",
                f.objects().count(),
                f.materials().count(),
                rig.joints.len(),
                rig.roots
            );
            let bytes = pac::member(&pac, motion_member)?;
            let mot = MotionArchive::from_directory(&bytes)?;
            for (i, g) in mot.groups.iter().enumerate() {
                let slots: Vec<_> = g
                    .motion_offsets
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| p.map(|_| i.to_string()))
                    .collect();
                println!(
                    "group {i}: {}/{} slots [{}]",
                    slots.len(),
                    g.motion_offsets.len(),
                    slots.join(",")
                );
            }
            Ok(())
        }
        Command::Package {
            monster,
            material_template,
            name,
            texture_member,
            motion_member,
            group,
            output,
        } => save(
            &output,
            package::build(
                &monster,
                &material_template,
                &name,
                texture_member,
                motion_member,
                group,
            )?,
        ),
        Command::HuntIcon { data, output } => {
            // Vita's hunting UI indexes the u16 table at mhfdat +1308, not
            // micon by species number. Ordinary em080 selects entry 70.
            let dat = pac::decode(read(&data.join("mhfdat.bin"))?)?;
            let table = u32::from_le_bytes(
                dat.get(1308..1312)
                    .context("missing icon table")?
                    .try_into()?,
            ) as usize;
            let pos = table + 2 * 80;
            let index = u16::from_le_bytes(
                dat.get(pos..pos + 2)
                    .context("missing Espinas icon index")?
                    .try_into()?,
            );
            let png = pac::member(&read(&data.join("micon128.bin"))?, index.into())?;
            let icon = mhf_resource::image::decode(&png)?;
            anyhow::ensure!(
                (icon.width, icon.height) == (128, 128),
                "invalid micon128 entry"
            );
            save(
                &output,
                mhgu_resource::tex::rgba8(icon.width, icon.height, &icon.rgba)?,
            )
        }
        Command::Hunt {
            pac,
            donor,
            material_template,
            vita_elf,
            output,
        } => save(
            &output,
            charge::hunt(&pac, &donor, &material_template, &vita_elf)?,
        ),
        Command::ChargeTest {
            pac,
            vita_elf,
            donor,
            material_template,
            output,
        } => save(
            &output,
            charge::build(&pac, &donor, &material_template, &vita_elf, &charge::CHARGE)?,
        ),
        Command::CombatReplay {
            pac,
            donor,
            material_template,
            vita_elf,
            distance,
            angle_units,
            enraged,
            random,
            output,
        } => {
            use mhf_resource::combat::{Combat, Target};
            anyhow::ensure!(
                distance.is_finite() && distance >= 0. && angle_units <= 32768,
                "expected nonnegative distance and absolute half-turn angle"
            );
            let src = read(&vita_elf)?;
            let seq = Combat::espinas(&src)?.requests(&Target {
                enraged,
                airborne: false,
                distance: Some(distance),
                angle: angle_units,
                random,
            })?;
            let action = charge::replay(&seq)?;
            println!(
                "FZ battle request {seq:?}, source motions {:?}",
                action.motions
            );
            save(
                &output,
                charge::build(&pac, &donor, &material_template, &vita_elf, &action)?,
            )
        }
        Command::CombatMotions { pac, donor, output } => {
            save(&output, charge::motion_bank(&pac, &donor)?)
        }
        Command::CombatChoices { input, output } => {
            use mhf_resource::combat::{Combat, Target};
            use std::fmt::Write;
            let src = read(&input)?;
            let combat = Combat::espinas(&src)?;
            let mut csv = String::from("enraged,airborne,distance,angle_units,rng5,requests\n");
            let mut probes = vec![None, Some(0.)];
            for d in combat.distances {
                probes.extend([Some(d), Some(d.next_up())]);
            }
            // The ordinary scripts compare against 20 degrees, rounded to 3641 units.
            for enraged in [false, true] {
                for airborne in [false, true] {
                    for &distance in &probes {
                        for angle in [3640, 3641] {
                            for random in 0..32 {
                                let seq = combat.requests(&Target {
                                    enraged,
                                    airborne,
                                    distance,
                                    angle,
                                    random,
                                })?;
                                let seq = seq
                                    .iter()
                                    .map(|(g, i)| format!("{g}:{i}"))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                writeln!(
                                    csv,
                                    "{},{},{},{angle},{random},{seq}",
                                    u8::from(enraged),
                                    u8::from(airborne),
                                    distance.map_or_else(|| "none".into(), |x| x.to_string())
                                )?;
                            }
                        }
                    }
                }
            }
            save(&output, csv.into_bytes())
        }
        Command::Emc {
            input,
            monster,
            bank,
            entry,
            action_sample,
            output,
        } => {
            let src = read(&input)?;
            let graph = ai::scan(&src, monster, bank, entry)?;
            if let Some(path) = output {
                let data = if action_sample {
                    let (data, info) = emc::sample(&graph)?;
                    println!("{info}");
                    data
                } else {
                    emc::convert(&graph)?
                };
                save(&path, data)
            } else {
                emc::report(&graph);
                Ok(())
            }
        }
    }
}
