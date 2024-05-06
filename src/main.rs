// Press B for benchmark.
// Preferably after frame time is reading consistently, rust-analyzer has calmed down, and with locked gpu clocks.

use std::{
    f32::consts::PI,
    ops::{Add, Mul, Sub},
    time::Instant,
};

mod auto_instance;
mod camera_controller;
mod mipmap_generator;

use argh::FromArgs;
use auto_instance::{AutoInstanceMaterialPlugin, AutoInstancePlugin};
use bevy::{
    core_pipeline::{
        bloom::BloomSettings,
        experimental::taa::{TemporalAntiAliasBundle, TemporalAntiAliasPlugin},
    },
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    pbr::{CascadeShadowConfigBuilder, ScreenSpaceAmbientOcclusionBundle},
    prelude::*,
    render::view::NoFrustumCulling,
    window::{PresentMode, WindowResolution},
    winit::{UpdateMode, WinitSettings},
};
use camera_controller::{CameraController, CameraControllerPlugin};
use mipmap_generator::{generate_mipmaps, MipmapGeneratorPlugin, MipmapGeneratorSettings};

use crate::light_consts::lux;
use crate::{
    auto_instance::{AutoInstanceMaterialRecursive, AutoInstanceMeshRecursive},
    convert::{change_gltf_to_use_ktx2, convert_images_to_ktx2},
};

mod convert;

#[derive(FromArgs, Resource, Clone)]
/// Config
pub struct Args {
    /// convert gltf to use ktx
    #[argh(switch)]
    convert: bool,

    /// enable auto instancing for meshes/materials
    #[argh(switch)]
    instance: bool,

    /// disable bloom, AO, AA, shadows
    #[argh(switch)]
    minimal: bool,

    /// whether to disable frustum culling.
    #[argh(switch)]
    no_frustum_culling: bool,
}

pub fn main() {
    let args: Args = argh::from_env();

    if args.convert {
        println!("This will take a few minutes");
        convert_images_to_ktx2();
        change_gltf_to_use_ktx2();
    }

    let mut app = App::new();

    app.insert_resource(args.clone())
        .insert_resource(Msaa::Off)
        .insert_resource(ClearColor(Color::rgb(1.75, 1.9, 1.99)))
        .insert_resource(AmbientLight {
            color: Color::rgb(1.0, 1.0, 1.0),
            brightness: 0.02,
        })
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                present_mode: PresentMode::Immediate,
                resolution: WindowResolution::new(1920.0, 1080.0).with_scale_factor_override(1.0),
                ..default()
            }),
            ..default()
        }))
        // Generating mipmaps takes a minute
        // Mipmap generation be skipped if ktx2 is used
        .insert_resource(MipmapGeneratorSettings {
            anisotropic_filtering: 16,
            ..default()
        })
        .add_plugins((
            LogDiagnosticsPlugin::default(),
            FrameTimeDiagnosticsPlugin,
            CameraControllerPlugin,
            MipmapGeneratorPlugin,
            TemporalAntiAliasPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                generate_mipmaps::<StandardMaterial>,
                proc_scene,
                input,
                benchmark,
                run_animation,
            ),
        );
    if args.no_frustum_culling {
        app.add_systems(Update, add_no_frustum_culling);
    }
    if args.instance {
        app.add_plugins((
            AutoInstancePlugin,
            AutoInstanceMaterialPlugin::<StandardMaterial>::default(),
        ));
    }

    app.run();
}

#[derive(Component)]
pub struct PostProcScene;

#[derive(Component)]
pub struct GrifLight;

pub fn setup(mut commands: Commands, asset_server: Res<AssetServer>, args: Res<Args>) {
    println!("Loading models, generating mipmaps");

    commands.spawn((
        SceneBundle {
            scene: asset_server.load("bistro_exterior/BistroExterior.gltf#Scene0"),
            ..default()
        },
        PostProcScene,
        AutoInstanceMaterialRecursive,
        AutoInstanceMeshRecursive,
    ));

    commands.spawn((
        SceneBundle {
            scene: asset_server.load("bistro_interior_wine/BistroInterior_Wine.gltf#Scene0"),
            ..default()
        },
        PostProcScene,
        AutoInstanceMaterialRecursive,
        AutoInstanceMeshRecursive,
    ));

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
                color: Color::rgb(1.0, 0.98, 0.96),
                illuminance: lux::FULL_DAYLIGHT,
                shadows_enabled: !args.minimal,
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

    // Camera
    let mut cam = commands.spawn((
        Camera3dBundle {
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
        },
        EnvironmentMapLight {
            diffuse_map: asset_server.load("environment_maps/pisa_diffuse_rgb9e5_zstd.ktx2"),
            specular_map: asset_server.load("environment_maps/pisa_specular_rgb9e5_zstd.ktx2"),
            intensity: 250.0,
        },
        CameraController::default().print_controls(),
    ));
    if !args.minimal {
        cam.insert((
            BloomSettings {
                intensity: 0.05,
                ..default()
            },
            TemporalAntiAliasBundle::default(),
        ))
        .insert(ScreenSpaceAmbientOcclusionBundle::default());
    }
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
    lights: Query<
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

                // Has a bunch of lights by default
                if lights.get(entity).is_ok() {
                    commands.entity(entity).despawn_recursive();
                }

                // Has a bunch of cameras by default
                if cameras.get(entity).is_ok() {
                    commands.entity(entity).despawn_recursive();
                }
            });
            commands.entity(entity).remove::<PostProcScene>();
        }
    }
}

const CAM_POS_1: Transform = Transform {
    translation: Vec3::new(-10.5, 1.7, -1.0),
    rotation: Quat::from_array([-0.05678932, 0.7372272, -0.062454797, -0.670351]),
    scale: Vec3::ONE,
};

const CAM_POS_2: Transform = Transform {
    translation: Vec3::new(24.149984, 1.9139149, -56.531208),
    rotation: Quat::from_array([-0.0006097495, -0.9720757, 0.0025259522, -0.23465316]),
    scale: Vec3::ONE,
};

const CAM_POS_3: Transform = Transform {
    translation: Vec3::new(2.1902895, 3.7706258, -9.204603),
    rotation: Quat::from_array([-0.04399063, -0.9307148, -0.119402625, 0.3428964]),
    scale: Vec3::ONE,
};

const ANIM_SPEED: f32 = 0.2;

const ANIM_CAM: [Transform; 3] = [
    Transform {
        translation: Vec3::new(-6.414026, 8.179898, -23.550516),
        rotation: Quat::from_array([-0.016413536, -0.88136566, -0.030704278, 0.4711502]),
        scale: Vec3::ONE,
    },
    Transform {
        translation: Vec3::new(-14.752817, 6.279289, 5.691277),
        rotation: Quat::from_array([-0.031593435, -0.516736, -0.019086324, 0.8553488]),
        scale: Vec3::ONE,
    },
    Transform {
        translation: Vec3::new(5.1539426, 8.142523, 16.436222),
        rotation: Quat::from_array([-0.07907656, -0.07581916, -0.006031934, 0.99396276]),
        scale: Vec3::ONE,
    },
];

fn input(input: Res<ButtonInput<KeyCode>>, mut camera: Query<&mut Transform, With<Camera>>) {
    let Ok(mut transform) = camera.get_single_mut() else {
        return;
    };
    if input.just_pressed(KeyCode::KeyI) {
        info!("{:?}", transform);
    }
    if input.just_pressed(KeyCode::Digit1) {
        *transform = CAM_POS_1
    }
    if input.just_pressed(KeyCode::Digit2) {
        *transform = CAM_POS_2
    }
    if input.just_pressed(KeyCode::Digit3) {
        *transform = CAM_POS_3
    }
}

fn lerp<T>(a: T, b: T, t: f32) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    a + (b - a) * t
}

fn follow_path(points: &[Transform], progress: f32) -> Transform {
    let total_segments = (points.len() - 1) as f32;
    let progress = progress.clamp(0.0, 1.0);
    let mut segment_progress = progress * total_segments;
    let segment_index = segment_progress.floor() as usize;
    segment_progress -= segment_index as f32;
    let a = points[segment_index];
    let b = points[(segment_index + 1).min(points.len() - 1)];
    Transform {
        translation: lerp(a.translation, b.translation, segment_progress),
        rotation: lerp(a.rotation, b.rotation, segment_progress),
        scale: lerp(a.scale, b.scale, segment_progress),
    }
}

fn run_animation(
    time: Res<Time>,
    input: Res<ButtonInput<KeyCode>>,
    mut animation_active: Local<bool>,
    mut camera: Query<&mut Transform, With<Camera>>,
) {
    let Ok(mut cam_tr) = camera.get_single_mut() else {
        return;
    };
    if input.just_pressed(KeyCode::Space) {
        *animation_active = !*animation_active;
    }
    if !*animation_active {
        return;
    }
    let progress = (time.elapsed_seconds() * ANIM_SPEED).fract();
    let cycle = 1.0 - (progress * 2.0 - 1.0).abs();
    let path_state = follow_path(&ANIM_CAM, cycle);
    // LPF
    cam_tr.translation = lerp(cam_tr.translation, path_state.translation, 0.1);
    cam_tr.rotation = lerp(cam_tr.rotation, path_state.rotation, 0.1);
}

fn benchmark(
    input: Res<ButtonInput<KeyCode>>,
    mut camera: Query<&mut Transform, With<Camera>>,
    mut bench_started: Local<Option<Instant>>,
    mut bench_frame: Local<u32>,
    mut count_per_step: Local<u32>,
    time: Res<Time>,
) {
    if input.just_pressed(KeyCode::KeyB) && bench_started.is_none() {
        *bench_started = Some(Instant::now());
        *bench_frame = 0;
        // Try to render for around 2s or at least 30 frames per step
        *count_per_step = ((2.0 / time.delta_seconds()) as u32).max(30);
        println!(
            "Starting Benchmark with {} frames per step",
            *count_per_step
        );
    }
    if bench_started.is_none() {
        return;
    }
    let Ok(mut transform) = camera.get_single_mut() else {
        return;
    };
    if *bench_frame == 0 {
        *transform = CAM_POS_1
    } else if *bench_frame == *count_per_step {
        *transform = CAM_POS_2
    } else if *bench_frame == *count_per_step * 2 {
        *transform = CAM_POS_3
    } else if *bench_frame == *count_per_step * 3 {
        let elapsed = bench_started.unwrap().elapsed().as_secs_f32();
        println!(
            "Benchmark avg cpu frame time: {:.2}ms",
            (elapsed / *bench_frame as f32) * 1000.0
        );
        *bench_started = None;
        *bench_frame = 0;
        *transform = CAM_POS_1;
    }
    *bench_frame += 1;
}

pub fn add_no_frustum_culling(
    mut commands: Commands,
    convert_query: Query<Entity, (Without<NoFrustumCulling>, With<Handle<StandardMaterial>>)>,
) {
    for entity in convert_query.iter() {
        commands.entity(entity).insert(NoFrustumCulling);
    }
}
