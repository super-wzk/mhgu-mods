mod hunter;
mod hunter_fx;
mod hunter_hud;
mod model;
mod motion;
mod package;
mod sound;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mhf_resource::{fmod::Fmod, motion::MotionArchive, pac, rig::Rig};
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
struct ModelArchive {
    /// Frontier model PAC
    pac: PathBuf,
    /// PAC member that contains the FMOD and FSKL pair
    #[arg(long, default_value_t = 0)]
    model_member: usize,
}
impl ModelArchive {
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
    /// Build the Extreme Gunlance action program and private GU resources.
    Gunlance {
        dat: PathBuf,
        #[arg(long)]
        gu_weapon: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build the Extreme Sword and Shield action program and private GU resources.
    SwordShield {
        dat: PathBuf,
        #[arg(long)]
        gu_weapon: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build the independent Frontier Switch Axe F motion, action, hit and sound resources.
    SwitchAxe {
        dat: PathBuf,
        #[arg(long)]
        gu_weapon: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert the local Frontier Magnet Spike motions and battle tables.
    MagnetSpikeBattle {
        dat: PathBuf,
        /// Also include the trial weapon model in magnet_spike.arc.
        #[arg(long)]
        material_template: Option<PathBuf>,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert the original long sword effect banks and animation sounds.
    LongSwordEffects {
        dat: PathBuf,
        #[arg(long)]
        gu_weapon: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    LongSwordSound {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert Frontier w07 motions and their equipment events.
    LongSwordMotions {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Extract the original long sword state, branch, event and contact tables.
    LongSwordActions {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert the paired Frontier dual-blade motion directories.
    DualBladesMotions {
        dat: PathBuf,
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
    /// Build the spirit and cut gauge backing for the long sword.
    LongSwordHud {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build the flame gauge and blade-polish sprite atlas.
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
        source: ModelArchive,
        #[arg(long, default_value_t = 2)]
        motion_member: usize,
    },
    /// Write a skinned MOD 0xD4 mesh with its skeleton and inverse bind matrices
    Model {
        #[command(flatten)]
        source: ModelArchive,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Write an LMT v67 animation list for one source MOT group
    Motion {
        #[command(flatten)]
        source: ModelArchive,
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
    /// Convert an original Frontier weapon model, skeleton and textures to a GU ARC.
    WeaponModel {
        archive: PathBuf,
        #[arg(long)]
        member: String,
        #[arg(long)]
        material_template: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Import the retail Magnet Spike icon from the Frontier UI atlas.
    MagnetSpikeIcon {
        dat: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Build a visual resource ARC (model, materials, textures and a motion group)
    Package {
        #[command(flatten)]
        source: ModelArchive,
        /// Native MRL or ARC with static opaque and cutout materials
        #[arg(long)]
        material_template: PathBuf,
        /// Target resource stem
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
        Command::GreatSwordActions { dat, output } => {
            save(&output, hunter::actions(&dat, hunter::Weapon::GreatSword)?)
        }
        Command::GreatSwordMotions { dat, output } => save(
            &output,
            hunter::motions(&dat, hunter::Weapon::GreatSword)?,
        ),
        Command::Gunlance { dat, gu_weapon, output } => {
            fs::create_dir_all(&output)?;
            let kind = hunter::Weapon::Gunlance;
            save(&output.join("motions.arc"), hunter::motions(&dat, kind)?)?;
            save(&output.join("gunlance.fgla"), hunter::actions(&dat, kind)?)?;
            let (assets, timeline) = sound::gunlance(&dat)?;
            save(&output.join("frontier_gl_sound.flse"), timeline)?;
            save(&output.join("sound.arc"), mhgu_resource::arc::encode(&assets)?)?;
            hunter_fx::gunlance(&dat, &output.join("effects"), &gu_weapon)
        }
        Command::SwordShield { dat, gu_weapon, output } => {
            fs::create_dir_all(&output)?;
            let kind = hunter::Weapon::SwordShield;
            save(&output.join("motions.arc"), hunter::motions(&dat, kind)?)?;
            save(&output.join("sword_shield.fsns"), hunter::actions(&dat, kind)?)?;
            hunter::blade_shapes(&dat, &output, &[6, 7, 23, 28, 29], 1)?;
            let (assets, timeline) = sound::sword_shield(&dat)?;
            save(&output.join("frontier_sns_sound.flse"), timeline)?;
            save(&output.join("sound.arc"), mhgu_resource::arc::encode(&assets)?)?;
            hunter_fx::sword_shield(&dat, &output.join("effects"), &gu_weapon)
        }
        Command::SwitchAxe { dat, gu_weapon, output } => {
            fs::create_dir_all(&output)?;
            save(&output.join("gauge.tex"),hunter_hud::switch_axe(&dat)?)?;
            hunter_fx::switch_axe(&dat, &output.join("effects"), &gu_weapon)?;
            save(
                &output.join("motions.arc"),
                hunter::motions(&dat, hunter::Weapon::SwitchAxe)?,
            )?;
            save(
                &output.join("switch_axe.fsaa"),
                hunter::actions(&dat, hunter::Weapon::SwitchAxe)?,
            )?;
            hunter::switch_axe_shapes(&dat, &output)?;
            let (assets, timeline) = sound::switch_axe(&dat)?;
            save(&output.join("frontier_sa_sound.flse"), timeline)?;
            save(
                &output.join("sound.arc"),
                mhgu_resource::arc::encode(&assets)?,
            )
        }
        Command::MagnetSpikeBattle {
            dat,
            material_template,
            output,
        } => {
            fs::create_dir_all(&output)?;
            let weapon = hunter::Weapon::MagnetSpike;
            let motions = hunter::motion_assets(&dat, weapon)?;
            let (archive, assets) = if let Some(template) = material_template {
                let mut appearance = package::weapon_assets(
                    &dat.join("extend/wk000.abn"),
                    "wk451.bin",
                    &template,
                    r"weapon\frontier\magnet_spike\wk451",
                )?;
                appearance.extend(motions);
                ("magnet_spike.arc", appearance)
            } else {
                ("motions.arc", motions)
            };
            let program = hunter::actions(&dat, weapon)?;
            save(&output.join(archive), mhgu_resource::arc::encode(&assets)?)?;
            save(&output.join("magnet_spike.fmsa"), program)
        }
        Command::LongSwordSound { dat, output } => {
            let (assets, timeline) = sound::long_sword(&dat)?;
            save(&output.with_extension("flse"), timeline)?;
            save(&output, mhgu_resource::arc::encode(&assets)?)
        }
        Command::LongSwordEffects {
            dat,
            gu_weapon,
            output,
        } => hunter_fx::long_sword(&dat, &output, &gu_weapon),
        Command::LongSwordMotions { dat, output } => save(
            &output,
            hunter::motions(&dat, hunter::Weapon::LongSword)?,
        ),
        Command::LongSwordActions { dat, output } => {
            hunter::blade_shapes(&dat, output.parent().unwrap_or(Path::new(".")), &[0, 32, 33], 3)?;
            save(&output, hunter::actions(&dat, hunter::Weapon::LongSword)?)
        }
        Command::DualBladesMotions { dat, output } => save(
            &output,
            hunter::motions(&dat, hunter::Weapon::DualBlades)?,
        ),
        Command::DualBladesActions { dat, output } => {
            save(&output, hunter::actions(&dat, hunter::Weapon::DualBlades)?)
        }
        Command::DualBladesSound { dat, output } => {
            let (assets, timeline) = sound::dual_blades(&dat)?;
            save(&output.with_extension("fdse"), timeline)?;
            save(&output, mhgu_resource::arc::encode(&assets)?)
        }
        Command::LongSwordHud { dat, output } => save(&output, hunter_hud::long_sword(&dat)?),
        Command::DualBladesHud { output } => save(&output, hunter_hud::dual_blades()?),
        Command::DualBladesEffects {
            dat,
            output,
            gu_weapon,
        } => hunter_fx::dual_blades(&dat, &output, &gu_weapon),
        Command::Model { source, output } => {
            let (_, pair, rig) = source.open()?;
            save(&output, model::convert(&pac::member(&pair, 0)?, &rig)?)
        }
        Command::Motion {
            source,
            motion_member,
            group,
            slot,
            root,
            tag,
            output,
        } => {
            let (pac, _, rig) = source.open()?;
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
            source,
            motion_member,
        } => {
            let (pac, pair, rig) = source.open()?;
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
        Command::WeaponModel {
            archive,
            member,
            material_template,
            name,
            output,
        } => save(
            &output,
            package::weapon(&archive, &member, &material_template, &name)?,
        ),
        Command::MagnetSpikeIcon { dat, output } => {
            // MHF-Z's 64 px weapon icon sheet is PNG member 54 in mhf.bin.
            let atlas = pac::member(&read(&dat.join("mhf.bin"))?, 54)?;
            let image = mhf_resource::image::decode(&atlas)?;
            anyhow::ensure!(
                (image.width, image.height) == (512, 1024),
                "unexpected Frontier weapon icon sheet"
            );
            let mut icon = Vec::with_capacity(64 * 64 * 4);
            for y in 896..960 {
                let start = (y * 512 + 192) * 4;
                icon.extend_from_slice(&image.rgba[start..start + 64 * 4]);
            }
            save(
                &output,
                mhgu_resource::tex::rgba8_unmipped(64, 64, &icon)?,
            )
        }
        Command::Package {
            source,
            material_template,
            name,
            texture_member,
            motion_member,
            group,
            output,
        } => save(
            &output,
            package::build(
                &source,
                &material_template,
                &name,
                texture_member,
                motion_member,
                group,
            )?,
        ),
    }
}
