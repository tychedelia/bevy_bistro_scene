use std::{f32::consts::PI, num::NonZeroU8, time::Duration};

mod camera_controller;
mod mipmap_generator;

use bevy::{
    asset::ChangeWatcher,
    core_pipeline::{bloom::BloomSettings, fxaa::Fxaa},
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    pbr::CascadeShadowConfigBuilder,
    prelude::*,
    window::PresentMode,
};
use camera_controller::{CameraController, CameraControllerPlugin};
use mipmap_generator::{generate_mipmaps, MipmapGeneratorPlugin, MipmapGeneratorSettings};

use crate::convert::{change_gltf_to_use_ktx2, convert_images_to_ktx2};

mod convert;

pub fn main() {
    let args = &mut std::env::args();
    args.next();
    if let Some(arg) = &args.next() {
        if arg == "--convert" {
            println!("This will take a few minutes");
            convert_images_to_ktx2();
            change_gltf_to_use_ktx2();
        }
    }

    let mut app = App::new();

    app.insert_resource(Msaa::Off)
        //.insert_resource(DefaultOpaqueRendererMethod(OpaqueRendererMethod::Forward))
        .insert_resource(ClearColor(Color::rgb(1.75, 1.9, 1.99)))
        .insert_resource(AmbientLight {
            color: Color::rgb(1.0, 1.0, 1.0),
            brightness: 0.02,
        })
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes: ChangeWatcher::with_delay(Duration::from_secs_f32(0.1)),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        present_mode: PresentMode::Immediate,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugin(LogDiagnosticsPlugin::default())
        .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(CameraControllerPlugin)
        // Generating mipmaps takes a minute
        .insert_resource(MipmapGeneratorSettings {
            anisotropic_filtering: NonZeroU8::new(16),
            ..default()
        })
        .add_plugin(MipmapGeneratorPlugin)
        // Mipmap generation be skipped if ktx2 is used
        .add_systems(Update, generate_mipmaps::<StandardMaterial>)
        .add_systems(Startup, setup)
        .add_systems(Update, (proc_scene, input));

    app.run();
}

#[derive(Component)]
pub struct PostProcScene;

#[derive(Component)]
pub struct GrifLight;

pub fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    println!("Loading models, generating mipmaps");

    commands
        .spawn(SceneBundle {
            scene: asset_server.load("bistro_exterior/BistroExterior.gltf#Scene0"),
            ..default()
        })
        .insert(PostProcScene);

    commands
        .spawn(SceneBundle {
            scene: asset_server.load("bistro_interior_wine/BistroInterior_Wine.gltf#Scene0"),
            ..default()
        })
        .insert(PostProcScene);

    // Sun
    commands
        .spawn(DirectionalLightBundle {
            transform: Transform::from_rotation(Quat::from_euler(
                EulerRot::XYZ,
                PI * -0.43,
                PI * -0.08,
                0.0,
            )),
            directional_light: DirectionalLight {
                color: Color::rgb(1.0, 1.0, 0.99),
                illuminance: 400000.0,
                shadows_enabled: true,
                shadow_depth_bias: 0.2,
                shadow_normal_bias: 0.2,
            },
            cascade_shadow_config: CascadeShadowConfigBuilder {
                num_cascades: 4,
                minimum_distance: 0.1,
                maximum_distance: 100.0,
                first_cascade_far_bound: 5.0,
                overlap_proportion: 0.2,
            }
            .into(),
            ..default()
        })
        .insert(GrifLight);

    let mut bloom_settings = BloomSettings::NATURAL;
    bloom_settings.intensity *= 0.35;
    // Camera
    commands
        .spawn((Camera3dBundle {
            camera: Camera {
                hdr: true,
                ..default()
            },
            transform: Transform::from_xyz(-10.5, 1.7, -1.0)
                .looking_at(Vec3::new(0.0, 3.5, 0.0), Vec3::Y),
            projection: Projection::Perspective(PerspectiveProjection {
                fov: std::f32::consts::PI / 3.0,
                near: 0.1,
                far: 1000.0,
                aspect_ratio: 1.0,
            }),
            ..default()
        },))
        .insert((
            CameraController::default().print_controls(),
            bloom_settings,
            Fxaa::default(),
            //NormalPrepass,
            //DepthPrepass,
            //MotionVectorPrepass,
            //DeferredPrepass,
        ));
}

pub fn all_children<F: FnMut(Entity)>(
    children: &Children,
    children_query: &Query<&Children>,
    closure: &mut F,
) {
    for child in children {
        if let Ok(children) = children_query.get(*child) {
            all_children(children, children_query, closure);
        }
        closure(*child);
    }
}

#[allow(clippy::type_complexity)]
pub fn proc_scene(
    mut commands: Commands,
    flip_normals_query: Query<Entity, With<PostProcScene>>,
    children_query: Query<&Children>,
    has_std_mat: Query<&Handle<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    _lights: Query<
        Entity,
        (
            Or<(With<PointLight>, With<DirectionalLight>, With<SpotLight>)>,
            Without<GrifLight>,
        ),
    >,
    cameras: Query<Entity, With<Camera>>,
) {
    for entity in flip_normals_query.iter() {
        if let Ok(children) = children_query.get(entity) {
            all_children(children, &children_query, &mut |entity| {
                // Sponza needs flipped normals
                if let Ok(mat_h) = has_std_mat.get(entity) {
                    if let Some(mat) = materials.get_mut(mat_h) {
                        mat.flip_normal_map_y = true;
                    }
                }

                // Sponza has a bunch of lights by default
                //if lights.get(entity).is_ok() {
                //    commands.entity(entity).despawn_recursive();
                //}

                // Sponza has a bunch of cameras by default
                if cameras.get(entity).is_ok() {
                    commands.entity(entity).despawn_recursive();
                }
            });
            commands.entity(entity).remove::<PostProcScene>();
        }
    }
}

fn input(input: Res<Input<KeyCode>>, mut camera: Query<(Entity, &mut Transform), With<Camera>>) {
    for (_, mut transform) in camera.iter_mut() {
        if input.just_pressed(KeyCode::I) {
            info!("{:?}", transform);
        }
        if input.just_pressed(KeyCode::Key1) {
            *transform = Transform {
                translation: Vec3::new(-10.5, 1.7, -1.0),
                rotation: Quat::from_array([-0.05678932, 0.7372272, -0.062454797, -0.670351]),
                scale: Vec3::new(1.0, 1.0, 1.0),
            }
        }
        if input.just_pressed(KeyCode::Key2) {
            *transform = Transform {
                translation: Vec3::new(24.149984, 1.9139149, -56.531208),
                rotation: Quat::from_array([-0.0006097495, -0.9720757, 0.0025259522, -0.23465316]),
                scale: Vec3::new(1.0, 1.0, 1.0),
            }
        }
        if input.just_pressed(KeyCode::Key3) {
            *transform = Transform {
                translation: Vec3::new(2.1902895, 3.7706258, -9.204603),
                rotation: Quat::from_array([-0.04399063, -0.9307148, -0.119402625, 0.3428964]),
                scale: Vec3::new(1.0, 1.0, 1.0),
            };
        }
    }
}