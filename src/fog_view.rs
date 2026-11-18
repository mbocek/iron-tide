use crate::abilities::Camouflage;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::catalog::Team;
use crate::components::{Building, Unit};
use crate::fog::{CellSight, FogOfWar};
use crate::local_view::LocalView;
use crate::map::{CrystalVisual, footprint_cells, map_cells};
use crate::scenery::Scenery;

const TEXELS_PER_CELL: i32 = 4;
const BLUR_RADIUS: i32 = 3;
fn resolution() -> u32 {
    (map_cells() * TEXELS_PER_CELL) as u32
}

fn texture_extent() -> Extent3d {
    Extent3d {
        width: resolution(),
        height: resolution(),
        depth_or_array_layers: 1,
    }
}
const UNEXPLORED_SHADE: u8 = 0;
/// Explored ground stays readable but clearly darker than what units can currently see.
const EXPLORED_SHADE: u8 = 184;
const VISIBLE_SHADE: u8 = 255;

/// The fog of war as a texture the terrain shader darkens the ground with.
#[derive(Resource)]
pub struct FogTexture(pub Handle<Image>);

/// Scenery hidden until the player explores its cell.
#[derive(Component)]
pub struct Shrouded;

/// An enemy structure the player has spotted at least once; it stays on the map afterwards.
#[derive(Component)]
pub struct Discovered;

pub fn create_fog_texture(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_fill(
        texture_extent(),
        TextureDimension::D2,
        &[VISIBLE_SHADE, VISIBLE_SHADE, VISIBLE_SHADE, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::all(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    commands.insert_resource(FogTexture(images.add(image)));
}

fn shade(sight: CellSight) -> u8 {
    match sight {
        CellSight::Unexplored => UNEXPLORED_SHADE,
        CellSight::Explored => EXPLORED_SHADE,
        CellSight::Visible => VISIBLE_SHADE,
    }
}

/// Softens the blocky cell grid with a separable box blur so the fog edge reads as a gradient.
fn blurred_shades(fog: &FogOfWar) -> Vec<u8> {
    let size = resolution() as i32;
    let at = |values: &[u32], x: i32, y: i32| {
        values[(y.clamp(0, size - 1) * size + x.clamp(0, size - 1)) as usize]
    };
    let raw: Vec<u32> = (0..size * size)
        .map(|i| shade(fog.sight_at(IVec2::new(i % size, i / size) / TEXELS_PER_CELL)) as u32)
        .collect();
    let taps = (2 * BLUR_RADIUS + 1) as u32;
    let horizontal: Vec<u32> = (0..size * size)
        .map(|i| {
            (-BLUR_RADIUS..=BLUR_RADIUS)
                .map(|d| at(&raw, i % size + d, i / size))
                .sum::<u32>()
                / taps
        })
        .collect();
    (0..size * size)
        .map(|i| {
            ((-BLUR_RADIUS..=BLUR_RADIUS)
                .map(|d| at(&horizontal, i % size, i / size + d))
                .sum::<u32>()
                / taps) as u8
        })
        .collect()
}

pub fn paint_fog_texture(
    view: LocalView,
    texture: Res<FogTexture>,
    mut painted_revision: Local<u64>,
    mut images: ResMut<Assets<Image>>,
) {
    let fog = view.sight();
    if *painted_revision == fog.revision {
        return;
    }
    let Some(mut image) = images.get_mut(&texture.0) else {
        return;
    };
    if image.texture_descriptor.size != texture_extent() {
        image.resize(texture_extent());
    }
    let Some(pixels) = image.data.as_mut() else {
        return;
    };
    for (texel, shade) in pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(blurred_shades(fog))
    {
        texel[..3].fill(shade);
    }
    *painted_revision = fog.revision;
}

/// Hides scenery and crystals on ground the player has not explored yet.
pub fn shroud_unexplored_scenery(
    mut commands: Commands,
    view: LocalView,
    scenery: Query<(Entity, &Transform), Or<(With<Scenery>, With<CrystalVisual>)>>,
) {
    for (entity, transform) in &scenery {
        if !view.sight().is_explored(transform.translation) {
            commands
                .entity(entity)
                .insert((Shrouded, Visibility::Hidden));
        }
    }
}

pub fn reveal_explored_scenery(
    mut commands: Commands,
    view: LocalView,
    mut seen_revision: Local<u64>,
    mut shrouded: Query<(Entity, &Transform, &mut Visibility), With<Shrouded>>,
) {
    let fog = view.sight();
    if *seen_revision == fog.revision {
        return;
    }
    *seen_revision = fog.revision;
    for (entity, transform, mut visibility) in &mut shrouded {
        if fog.is_explored(transform.translation) {
            *visibility = Visibility::Inherited;
            commands.entity(entity).remove::<Shrouded>();
        }
    }
}

/// Enemy units show only where the player can see; enemy structures appear once spotted and then stay.
pub fn show_seen_enemies(
    mut commands: Commands,
    view: LocalView,
    mut units: Query<(&Team, &Transform, &mut Visibility, Option<&Camouflage>), With<Unit>>,
    mut buildings: Query<
        (Entity, &Team, &Building, &mut Visibility, Has<Discovered>),
        Without<Unit>,
    >,
) {
    for (&team, transform, mut visibility, camouflage) in &mut units {
        let hidden_by_camouflage = camouflage.is_some_and(|camouflage| camouflage.concealed)
            && view.is_hostile(team)
            && !view.is_spectating();
        let wanted = if view.reveals(team, transform.translation) && !hidden_by_camouflage {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(wanted);
    }
    for (entity, &team, building, mut visibility, discovered) in &mut buildings {
        if !view.is_hostile(team) || discovered {
            continue;
        }
        let spotted = footprint_cells(building.origin, building.kind.spec().footprint)
            .any(|cell| view.sight().sight_at(cell) == CellSight::Visible);
        if spotted {
            commands.entity(entity).insert(Discovered);
            *visibility = Visibility::Inherited;
        } else {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}
