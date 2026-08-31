//! simple-3d-viewer — a tiny GLB/glTF/OBJ/STL/3MF viewer written in Rust.
//!
//! Usage: `simple-3d-viewer [model.glb]` (no argument opens a file dialog).
//!
//! Controls: drag = orbit · right-drag = pan · scroll = zoom
//!           W = wireframe · Space = auto-rotate · R = reset camera
//!           O = open another model · H = help · Esc = quit
//!
//! A model file can also be dragged onto the window, and the open file is
//! hot-reloaded when it changes on disk. `--register` / `--unregister` set up
//! (or remove) the per-user file association so double-clicking a model file
//! opens it directly.

// a GUI double-click must not leave a console window behind; when launched from
// a terminal, the platform layer re-attaches to it so progress lines still show
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use three_d::*;

#[cfg(windows)]
mod windows;

mod registry;

const SETTLE: Duration = Duration::from_millis(700);
const EXTENSIONS: [&str; 5] = ["glb", "gltf", "obj", "stl", "3mf"];

fn main() {
    let mut args = std::env::args().skip(1);
    let mut selftest = false;
    let mut screenshot: Option<String> = None;
    let mut path: Option<String> = None;
    let mut action: Option<registry::Action> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--selftest" => selftest = true,
            "--screenshot" => {
                screenshot = Some(args.next().unwrap_or_else(|| {
                    eprintln!("--screenshot requires an output path, e.g. --screenshot out.png");
                    std::process::exit(2);
                }))
            }
            "--register" => action = Some(registry::Action::Register),
            "--unregister" => action = Some(registry::Action::Unregister),
            _ => path = Some(a),
        }
    }

    let action = match action {
        Some(a) => ViewerAction::Run(a),
        None => ViewerAction::View,
    };
    match action {
        ViewerAction::Run(a) => {
            let exe = std::env::current_exe()
                .map_err(|e| format!("cannot find my own path: {e}"))
                .and_then(|exe| registry::run(a, &exe).map(|_| {
                    println!("done");
                }));
            if let Err(e) = exe {
                report_error(&e);
                std::process::exit(1);
            }
            return;
        }
        ViewerAction::View => {}
    }

    platform_init();
    if let Err(e) = run(selftest, screenshot, path) {
        report_error(&e);
        std::process::exit(1);
    }
}

enum ViewerAction {
    View,
    Run(registry::Action),
}

fn run(selftest: bool, screenshot: Option<String>, path: Option<String>) -> Result<(), String> {
    let path = match path {
        Some(p) => p,
        None => match pick_file(None) {
            Some(p) => p.to_string_lossy().to_string(),
            None => return Ok(()), // cancelled — not an error
        },
    };

    let (ww, wh) = initial_size();
    let window = Window::new(WindowSettings {
        title: "simple-3d-viewer".to_string(),
        min_size: (900, 640),
        max_size: Some((ww, wh)),
        ..Default::default()
    })
    .map_err(|e| format!("failed to create window: {e}"))?;
    let context = window.gl();
    let viewport = window.viewport();

    #[cfg(windows)]
    let drop_target = windows::DropTarget::register();
    #[cfg(not(windows))]
    let drop_target: Option<std::rc::Rc<()>> = None;

    let loaded = load_model(&context, &path)?;
    let mut app = App::from_loaded(loaded, viewport, None);
    app.path = PathBuf::from(&path);
    if let Some(mt) = file_modified(&app.path) {
        app.mtime = mt;
    }
    app.title();

    let ambient = AmbientLight::new(&context, 0.25, Srgba::WHITE);
    let key = DirectionalLight::new(&context, 0.85, Srgba::WHITE, vec3(-0.6, -1.0, -0.5));
    let fill = DirectionalLight::new(
        &context,
        0.3,
        Srgba::new(150, 175, 205, 255),
        vec3(0.8, -0.1, 0.6),
    );

    let app = Rc::new(RefCell::new(app));
    let ctx = context;
    let shot = screenshot;

    window.render_loop(move |mut frame_input| {
        let a = &mut *app.borrow_mut();
        let mut redraw = frame_input.first_frame;

        // --- file drag & drop onto the window ---
        #[cfg(windows)]
        if let Some(dt) = &drop_target {
            for p in dt.take_drops() {
                match load_model(&ctx, &p.to_string_lossy()) {
                    Ok(l) => {
                        a.replace_model(l, frame_input.viewport, Some(p));
                        redraw = true;
                    }
                    Err(e) => report_error(&e),
                }
            }
        }
        #[cfg(not(windows))]
        let _ = &drop_target;

        // --- hot reload when the open file changes on disk ---
        a.poll_reload(&ctx);

        // --- input ---
        redraw |= a.camera.set_viewport(frame_input.viewport);
        a.viewport = frame_input.viewport;
        redraw |= a.control.handle_events(&mut a.camera, &mut frame_input.events);

        for event in frame_input.events.iter() {
            match event {
                Event::MouseWheel { delta, .. } => {
                    if !a.spin {
                        a.spin_vel = (a.spin_vel + (delta.0 + delta.1) * 0.012).clamp(-0.6, 0.6);
                        redraw = true;
                    }
                }
                Event::KeyPress { kind, .. } => match kind {
                    Key::W => {
                        a.wire_on = !a.wire_on;
                        redraw = true;
                    }
                    Key::Space => {
                        a.spin = !a.spin;
                        redraw = true;
                    }
                    Key::R => {
                        a.fit_camera(frame_input.viewport);
                        redraw = true;
                    }
                    Key::O => {
                        if let Some(p) = pick_file(a.dialog_dir()) {
                            match load_model(&ctx, &p.to_string_lossy()) {
                                Ok(l) => {
                                    a.replace_model(l, frame_input.viewport, Some(p));
                                    redraw = true;
                                }
                                Err(e) => report_error(&e),
                            }
                        }
                    }
                    Key::H => {
                        a.help_until = Some(Instant::now() + Duration::from_secs(8));
                        a.title();
                        redraw = true;
                    }
                    Key::Escape => {
                        return FrameOutput {
                            exit: true,
                            ..Default::default()
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        // --- auto-rotate (camera orbits the model, so arbitrary node transforms
        // stay intact); scroll nudges the spin while it is off ---
        if a.spin {
            a.orbit(Duration::from_millis(frame_input.elapsed_time.max(0.0) as u64));
            redraw = true;
        } else if a.spin_vel.abs() > 0.0002 {
            a.orbit(Duration::from_millis(frame_input.elapsed_time.max(0.0) as u64));
            a.spin_vel *= 0.92;
            redraw = true;
        }
        a.title_due();

        // --- render ---
        let mut objects: Vec<&dyn Object> = Vec::new();
        for m in &a.models {
            objects.push(m);
        }
        if a.wire_on {
            for w in &a.wires {
                objects.push(w);
            }
        }

        if redraw {
            frame_input
                .screen()
                .clear(ClearState::color_and_depth(0.10, 0.11, 0.13, 1.0, 1.0))
                .render(&a.camera, objects.iter().copied(), &[&ambient, &key, &fill]);

            a.frames += 1;
            let exit_now = (selftest && a.frames == 5) || (shot.is_some() && a.frames == 10);
            if exit_now {
                let vp = frame_input.viewport;
                let (w, h) = (vp.width as usize, vp.height as usize);
                let px = frame_input.screen().read_color::<[u8; 4]>();

                if let Some(out) = &shot {
                    let buf: Vec<u8> = px.iter().flat_map(|c| c.iter()).copied().collect();
                    match image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                        Some(img) => match img.save(out) {
                            Ok(()) => println!("screenshot written to {out}"),
                            Err(e) => eprintln!("failed to write {out}: {e}"),
                        },
                        None => eprintln!("failed to encode screenshot"),
                    }
                    return FrameOutput {
                        exit: true,
                        ..Default::default()
                    };
                }

                // selftest: anything that is not (approximately) the clear color
                // means the model actually rendered
                let covered = px.iter().filter(|c| c[0].abs_diff(25) > 2).count();
                let pct = covered as f32 / px.len().max(1) as f32 * 100.0;
                if covered == 0 {
                    eprintln!("selftest FAILED: nothing but the clear color on screen");
                } else {
                    println!("selftest PASSED: model covers {pct:.1}% of the viewport");
                }
                return FrameOutput {
                    exit: true,
                    ..Default::default()
                };
            }
        }

        FrameOutput {
            swap_buffers: redraw,
            ..Default::default()
        }
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// model loading
// ---------------------------------------------------------------------------

struct Loaded {
    file_name: String,
    verts: usize,
    tris: usize,
    meshes: usize,
    center: Vec3,
    radius: f32,
    models: Vec<Gm<Mesh, PhysicalMaterial>>,
    wires: Vec<Wireframe>,
}

fn load_model(ctx: &Context, path: &str) -> Result<Loaded, String> {
    let file_name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    println!("loading {file_name} ...");
    let mut loaded = three_d_asset::io::load(&[path])
        .map_err(|e| format!("failed to load {file_name}: {e}"))?;
    let stem = Path::new(path)
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".to_string());
    let mut cpu_model: three_d_asset::Model = loaded
        .deserialize(&stem)
        .map_err(|e| format!("failed to parse {file_name}: {e}"))?;
    cpu_model.geometries.iter_mut().for_each(|g| {
        // a viewer needs normals to shade, and tangents are only needed for
        // normal-mapped models and require uvs to exist
        if let three_d_asset::Geometry::Triangles(t) = &mut **g {
            if t.normals.is_none() {
                t.compute_normals();
            }
            if t.uvs.is_some() {
                t.compute_tangents();
            }
        }
    });

    let mut tris = 0usize;
    let mut verts = 0usize;
    for p in &cpu_model.geometries {
        if let three_d_asset::Geometry::Triangles(t) = &p.geometry {
            verts += t.vertex_count();
            tris += t.triangle_count();
        }
    }
    if tris == 0 {
        return Err(format!("{file_name} contains no triangle geometry"));
    }
    println!("ok: {verts} vertices, {tris} triangles, {} mesh(es)", cpu_model.geometries.len());

    // a mesh without a material (common for geometry-only exports like Hunyuan3D
    // GLBs) gets a neutral clay look — the glTF spec default is fully metallic,
    // which renders near-black without an environment map
    let clay = three_d_asset::PbrMaterial {
        albedo: Srgba::new_opaque(195, 195, 195),
        roughness: 0.65,
        metallic: 0.0,
        ..Default::default()
    };
    // upload meshes
    let mut models: Vec<Gm<Mesh, PhysicalMaterial>> = Vec::new();
    for p in &cpu_model.geometries {
        if let three_d_asset::Geometry::Triangles(t) = &p.geometry {
            let cpu_mat = p.material_index.and_then(|i| cpu_model.materials.get(i));
            let material = PhysicalMaterial::from_cpu_material(ctx, cpu_mat.unwrap_or(&clay));
            let mesh = Mesh::new(ctx, t);
            let mut gm = Gm::new(mesh, material);
            gm.set_transformation(p.transformation);
            models.push(gm);
        }
    }

    let wireframes =
        Wireframe::new_from_cpu_model(ctx, &cpu_model, 1.0, Srgba::new(110, 200, 250, 255));

    // bounding box over all meshes (transformed corners), used to fit the camera
    let mut bbox: Option<(Vec3, Vec3)> = None;
    for p in cpu_model.geometries.iter() {
        if let three_d_asset::Geometry::Triangles(t) = &p.geometry {
            let mesh = Mesh::new(ctx, t);
            let a = mesh.aabb();
            for cx in [a.min().x, a.max().x] {
                for cy in [a.min().y, a.max().y] {
                    for cz in [a.min().z, a.max().z] {
                        let c = p
                            .transformation
                            .transform_point(Point3::new(cx, cy, cz))
                            .to_vec();
                        bbox = Some(match bbox {
                            None => (c, c),
                            Some((mn, mx)) => (
                                vec3(mn.x.min(c.x), mn.y.min(c.y), mn.z.min(c.z)),
                                vec3(mx.x.max(c.x), mx.y.max(c.y), mx.z.max(c.z)),
                            ),
                        });
                    }
                }
            }
        }
    }
    let (mn, mx) = bbox.unwrap_or((vec3(-1.0, -1.0, -1.0), vec3(1.0, 1.0, 1.0)));
    let center = (mn + mx) * 0.5;
    let radius = ((mx - mn).magnitude() * 0.5).max(0.001);

    Ok(Loaded {
        file_name,
        verts,
        tris,
        meshes: cpu_model.geometries.len(),
        center,
        radius,
        models,
        wires: wireframes,
    })
}

// ---------------------------------------------------------------------------
// viewer state
// ---------------------------------------------------------------------------

struct App {
    path: PathBuf,
    mtime: i64,
    last_write: Option<Instant>,
    models: Vec<Gm<Mesh, PhysicalMaterial>>,
    wires: Vec<Wireframe>,
    center: Vec3,
    radius: f32,
    camera: Camera,
    control: OrbitControl,
    viewport: Viewport,
    spin: bool,
    spin_vel: f32,
    wire_on: bool,
    frames: u32,
    title_base: String,
    help_until: Option<Instant>,
    last_title: String,
    last_dialog_dir: Option<PathBuf>,
}

impl App {
    fn from_loaded(l: Loaded, viewport: Viewport, dir: Option<PathBuf>) -> Self {
        let (camera, control) = fit_camera(viewport, l.center, l.radius);
        Self {
            path: PathBuf::new(),
            mtime: 0,
            last_write: None,
            models: l.models,
            wires: l.wires,
            center: l.center,
            radius: l.radius,
            camera,
            control,
            viewport,
            spin: true,
            spin_vel: 0.0,
            wire_on: false,
            frames: 0,
            title_base: format!("{} ({} verts / {} tris · {} mesh)", l.file_name, l.verts, l.tris, l.meshes),
            help_until: None,
            last_title: String::new(),
            last_dialog_dir: dir,
        }
    }

    fn replace_model(&mut self, l: Loaded, viewport: Viewport, file: Option<PathBuf>) {
        self.title_base =
            format!("{} ({} verts / {} tris · {} mesh)", l.file_name, l.verts, l.tris, l.meshes);
        self.center = l.center;
        self.radius = l.radius;
        self.models = l.models;
        self.wires = l.wires;
        self.fit_camera(viewport);
        if let Some(f) = file {
            self.path = f;
            self.mtime = file_modified(&self.path).unwrap_or(0);
            self.last_write = None;
        }
        self.title();
    }

    fn fit_camera(&mut self, viewport: Viewport) {
        let (camera, control) = fit_camera(viewport, self.center, self.radius);
        self.camera = camera;
        self.control = control;
    }

    fn dialog_dir(&self) -> Option<&Path> {
        self.last_dialog_dir
            .as_deref()
            .or(self.path.parent())
    }

    fn orbit(&mut self, elapsed: Duration) {
        let delta = Rad(elapsed.as_secs_f32() * 0.18);
        let center = self.center;
        let target = self.camera.target();
        let rel = self.camera.position() - center;
        self.camera.set_view(
            center + Mat4::from_axis_angle(vec3(0.0, 1.0, 0.0), delta).transform_vector(rel),
            target,
            vec3(0.0, 1.0, 0.0),
        );
    }

    fn title(&mut self) {
        let mut t = format!("simple-3d-viewer — {}", self.title_base);
        if let Some(h) = self.help_until {
            if Instant::now() < h {
                t.push_str("   [W] wire · [Space] spin · [R] reset · [O] open · [drag] orbit · [right-drag] pan · [scroll] zoom · [H] help · [Esc] quit");
            } else {
                self.help_until = None;
            }
        }
        if !self.spin {
            t.push_str("   [Space] auto-rotate off — scroll to nudge");
        }
        self.last_title = t.clone();
        set_window_title(&t);
    }

    fn title_due(&mut self) {
        // refresh the "help" hint expiry without spamming SetWindowText
        let want_help = self.help_until.map(|h| Instant::now() < h).unwrap_or(false);
        if self.help_until.is_some() && !want_help {
            self.help_until = None;
            self.title();
        }
    }

    fn poll_reload(&mut self, ctx: &Context) {
        let now = Instant::now();
        if let Some(lw) = self.last_write {
            if now >= lw + SETTLE {
                self.last_write = None;
                match load_model(ctx, &self.path.to_string_lossy()) {
                    Ok(l) => {
                        let vp = self.viewport;
                        self.replace_model(l, vp, None);
                    }
                    Err(e) => eprintln!("reload failed: {e}"),
                }
            }
            return;
        }
        if self.path.as_os_str().is_empty() {
            return;
        }
        if let Some(mt) = file_modified(&self.path) {
            if self.mtime == 0 {
                self.mtime = mt;
            } else if mt != self.mtime {
                self.mtime = mt;
                self.last_write = Some(now);
            }
        }
    }
}

fn fit_camera(viewport: Viewport, center: Vec3, radius: f32) -> (Camera, OrbitControl) {
    let dist = radius * 2.4;
    let camera = Camera::new_perspective(
        viewport,
        center + vec3(0.62, 0.42, 0.78).normalize() * dist,
        center,
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        dist * 0.01,
        dist * 40.0,
    );
    let control = OrbitControl::new(camera.target(), dist * 0.1, dist * 20.0);
    (camera, control)
}

// ---------------------------------------------------------------------------
// cross-platform helpers (dialogs, errors, file watching)
// ---------------------------------------------------------------------------

fn pick_file(dir: Option<&Path>) -> Option<PathBuf> {
    let mut dlg = rfd::FileDialog::new()
        .add_filter("3D models", &EXTENSIONS)
        .set_title("Pick a model to peek at");
    if let Some(d) = dir {
        dlg = dlg.set_directory(d);
    }
    dlg.pick_file()
}

#[cfg(windows)]
fn report_error(msg: &str) {
    eprintln!("error: {msg}");
    windows::message_box(msg);
}

#[cfg(not(windows))]
fn report_error(msg: &str) {
    eprintln!("error: {msg}");
    rfd::MessageDialog::new()
        .set_title("simple-3d-viewer")
        .set_description(msg)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

#[cfg(windows)]
fn platform_init() {
    windows::attach_console();
}

#[cfg(not(windows))]
fn platform_init() {}

#[cfg(windows)]
fn set_window_title(t: &str) {
    windows::set_title(t);
}

#[cfg(not(windows))]
fn set_window_title(_t: &str) {}

#[cfg(windows)]
fn file_modified(p: &Path) -> Option<i64> {
    windows::file_modified(p)
}

#[cfg(not(windows))]
fn file_modified(p: &Path) -> Option<i64> {
    std::fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

#[cfg(windows)]
fn initial_size() -> (u32, u32) {
    windows::initial_size()
}

#[cfg(not(windows))]
fn initial_size() -> (u32, u32) {
    (1600, 900)
}
