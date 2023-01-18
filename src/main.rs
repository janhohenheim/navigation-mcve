use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_editor_pls::prelude::*;
use bevy_prototype_debug_lines::{DebugLines, DebugLinesPlugin};
use bevy_rapier3d::prelude::*;
use oxidized_navigation::query::{find_path, perform_string_pulling_on_path};
use oxidized_navigation::{NavMesh, NavMeshAffector, NavMeshSettings, OxidizedNavigationPlugin};
use smooth_bevy_cameras::{
    controllers::unreal::{UnrealCameraBundle, UnrealCameraController, UnrealCameraPlugin},
    LookTransformPlugin,
};
use std::f32::consts::TAU;
use std::iter;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            window: WindowDescriptor {
                width: 800.,
                height: 600.,
                title: "Bevy game".to_string(),
                canvas: Some("#bevy".to_owned()),
                present_mode: PresentMode::AutoVsync,
                ..default()
            },
            ..default()
        }))
        .add_plugin(DebugLinesPlugin::default())
        .add_plugin(OxidizedNavigationPlugin)
        .add_plugin(EditorPlugin)
        .add_plugin(LookTransformPlugin)
        .add_plugin(UnrealCameraPlugin::default())
        .add_plugin(RapierPhysicsPlugin::<NoUserData>::default())
        .add_plugin(RapierDebugRenderPlugin::default())
        .insert_resource(RapierConfiguration::default())
        .insert_resource(NavMeshSettings {
            cell_width: 0.25,
            cell_height: 0.1,
            tile_width: 100,
            world_half_extents: 250.0,
            world_bottom_bound: -10.0,
            max_traversable_slope_radians: (40.0_f32 - 0.1).to_radians(),
            walkable_height: 20,
            walkable_radius: 2,
            step_height: 3,
            min_region_area: 100,
            merge_region_area: 500,
            max_contour_simplification_error: 0.5,
            max_edge_length: 80,
        })
        .insert_resource(Msaa { samples: 4 })
        .insert_resource(ClearColor(Color::rgb(0.4, 0.4, 0.4)))
        .add_startup_system(setup)
        .add_system(spawn_scene)
        .add_system(draw_nav_mesh)
        .add_system(read_colliders)
        .add_system(navigate)
        .run();
}

#[derive(Resource)]
struct ToSpawn(Handle<Gltf>);

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    let gltf_handle = asset_server.load("level.glb");
    commands.insert_resource(ToSpawn(gltf_handle));

    commands
        .spawn(Camera3dBundle::default())
        .insert(UnrealCameraBundle::new(
            UnrealCameraController::default(),
            Vec3::new(-2.0, 5.0, 5.0),
            Vec3::new(0., 0., 0.),
        ));
}

fn spawn_scene(
    mut commands: Commands,
    gltf_assets: Res<Assets<Gltf>>,
    to_spawn: Option<Res<ToSpawn>>,
) {
    if let Some(to_spawn) = to_spawn {
        if let Some(gltf) = gltf_assets.get(&to_spawn.0) {
            info!("spawned level");
            commands.spawn((SceneBundle {
                scene: gltf.scenes[0].clone(),
                ..default()
            },));
            commands.remove_resource::<ToSpawn>();

            const HALF_SIZE: f32 = 50.0;
            commands.spawn((
                DirectionalLightBundle {
                    directional_light: DirectionalLight {
                        // Configure the projection to better fit the scene
                        shadow_projection: OrthographicProjection {
                            left: -HALF_SIZE,
                            right: HALF_SIZE,
                            bottom: -HALF_SIZE,
                            top: HALF_SIZE,
                            near: -10.0 * HALF_SIZE,
                            far: 10.0 * HALF_SIZE,
                            ..default()
                        },
                        shadows_enabled: true,
                        ..default()
                    },
                    transform: Transform {
                        translation: Vec3::new(0.0, 2.0, 0.0),
                        rotation: Quat::from_rotation_x(-TAU / 8.),
                        ..default()
                    },
                    ..default()
                },
                Name::new("Light"),
            ));
        }
    }
}

pub fn read_colliders(
    mut commands: Commands,
    added_name: Query<(Entity, &Name, &Children), Added<Name>>,
    meshes: Res<Assets<Mesh>>,
    mesh_handles: Query<&Handle<Mesh>>,
) {
    for (entity, name, children) in &added_name {
        if name.to_lowercase().contains("collider") {
            let colliders: Vec<_> = children
                .iter()
                .filter_map(|entity| mesh_handles.get(*entity).ok().map(|mesh| (*entity, mesh)))
                .collect();
            let (collider_entity, collider_mesh_handle) = colliders.first().unwrap();
            let collider_mesh = meshes.get(collider_mesh_handle).unwrap();
            commands.entity(*collider_entity).despawn_recursive();

            let rapier_collider =
                Collider::from_bevy_mesh(collider_mesh, &ComputedColliderShape::TriMesh).unwrap();

            commands
                .entity(entity)
                .insert((rapier_collider, NavMeshAffector::default()));
        }
    }
}

fn navigate(
    nav_mesh_settings: Res<NavMeshSettings>,
    nav_mesh: Res<NavMesh>,
    mut lines: ResMut<DebugLines>,
    camera_transform: Query<&Transform, With<UnrealCameraController>>,
) {
    if let Ok(nav_mesh) = nav_mesh.get().read() {
        let start_pos = (0., 0., 0.).into();
        let end_pos = camera_transform.iter().next().unwrap().translation;

        // Run pathfinding to get a polygon path.
        match find_path(&nav_mesh, &nav_mesh_settings, start_pos, end_pos, Some(5.)) {
            Ok(path) => {
                // Convert polygon path to a path of Vec3s.
                match perform_string_pulling_on_path(&nav_mesh, start_pos, end_pos, &path) {
                    Ok(string_path) => {
                        let path = iter::once(start_pos)
                            .chain(string_path.into_iter())
                            .chain(iter::once(end_pos));
                        for (a, b) in path.clone().zip(path.skip(1)) {
                            lines.line_colored(a, b, 0., Color::RED);
                        }
                    }
                    Err(error) => error!("Error with string path: {:?}", error),
                };
            }
            Err(error) => error!("Error with pathfinding: {:?}", error),
        }
    }
}

fn draw_nav_mesh(nav_mesh: Res<NavMesh>, mut lines: ResMut<DebugLines>) {
    if let Ok(nav_mesh) = nav_mesh.get().read() {
        for (_, tile) in nav_mesh.get_tiles().iter() {
            // Draw polygons.
            for poly in tile.polygons.iter() {
                let indices = &poly.indices;
                for i in 0..indices.len() {
                    let a = tile.vertices[indices[i] as usize];
                    let b = tile.vertices[indices[(i + 1) % indices.len()] as usize];
                    lines.line(a, b, 0.0);
                }
            }

            // Draw vertex points.
            for vertex in tile.vertices.iter() {
                lines.line(*vertex, *vertex + Vec3::Y, 0.0);
            }
        }
    }
}
