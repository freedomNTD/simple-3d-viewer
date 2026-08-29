//! glb-peek — a tiny GLB/glTF viewer written in Rust.
//!
//! Usage: `glb-peek [model.glb]` (no argument opens a file dialog).
//!
//! Controls: drag = orbit · right-drag = pan · scroll = zoom
//!           W = wireframe · Space = auto-rotate · Esc = quit

use std::path::Path;

use three_d::*;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut selftest = false;
    let mut screenshot: Option<String> = None;
    let mut path = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--selftest" => selftest = true,
            "--screenshot" => {
                screenshot = Some(args.next().unwrap_or_else(|| {
                    eprintln!("--screenshot requires an output path, e.g. --screenshot out.png");
                    std::process::exit(2);
                }))
            }
            _ => path = Some(a),
        }
    }
    let path = match path {
        Some(p) => p,
        None => match rfd::FileDialog::new()
            .add_filter("glTF models", &["glb", "gltf"])
            .set_title("Pick a model to peek at")
            .pick_file()
        {
            Some(p) => p.display().to_string(),
            None => return Ok(()), // cancelled — not an error
        },
    };

    let file_name = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    println!("loading {file_name} ...");
    let mut loaded = three_d_asset::io::load_async(&[path.clone()])
        .await
        .map_err(|e| format!("failed to load {file_name}: {e}"))?;
    let stem = Path::new(&path)
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
    println!(
        "ok: {verts} vertices, {tris} triangles, {} mesh(es)",
        cpu_model.geometries.len()
    );

    let window = Window::new(WindowSettings {
        title: format!(
            "glb-peek — {file_name} ({verts} verts / {tris} tris)  [W wireframe · Space spin · Esc quit]"
        ),
        min_size: (900, 640),
        ..Default::default()
    })
    .map_err(|e| format!("failed to create window: {e}"))?;
    let context = window.gl();

    // upload meshes; a mesh without a material (common for geometry-only exports
    // like Hunyuan3D GLBs) gets a neutral clay look — the glTF spec default is
    // fully metallic, which renders near-black without an environment map
    let clay = three_d_asset::PbrMaterial {
        albedo: Srgba::new_opaque(195, 195, 195),
        roughness: 0.65,
        metallic: 0.0,
        ..Default::default()
    };
    let mut models: Vec<Gm<Mesh, PhysicalMaterial>> = Vec::new();
    for p in &cpu_model.geometries {
        if let three_d_asset::Geometry::Triangles(t) = &p.geometry {
            let cpu_mat = p.material_index.and_then(|i| cpu_model.materials.get(i));
            let material = PhysicalMaterial::from_cpu_material(&context, cpu_mat.unwrap_or(&clay));
            let mut gm = Gm::new(Mesh::new(&context, t), material);
            gm.set_transformation(p.transformation);
            models.push(gm);
        }
    }
    let wireframes =
        Wireframe::new_from_cpu_model(&context, &cpu_model, 1.0, Srgba::new(110, 200, 250, 255));

    // bounding box over all meshes (transformed corners), used to fit the camera
    let mut bbox: Option<(Vec3, Vec3)> = None;
    for p in &cpu_model.geometries {
        if let three_d_asset::Geometry::Triangles(t) = &p.geometry {
            let mesh = Mesh::new(&context, t);
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
    let dist = radius * 2.4;

    let mut camera = Camera::new_perspective(
        window.viewport(),
        center + vec3(0.62, 0.42, 0.78).normalize() * dist,
        center,
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        dist * 0.01,
        dist * 40.0,
    );
    let mut control = OrbitControl::new(camera.target(), dist * 0.1, dist * 20.0);

    let ambient = AmbientLight::new(&context, 0.25, Srgba::WHITE);
    let key = DirectionalLight::new(&context, 0.85, Srgba::WHITE, vec3(-0.6, -1.0, -0.5));
    let fill = DirectionalLight::new(&context, 0.3, Srgba::new(150, 175, 205, 255), vec3(0.8, -0.1, 0.6));

    let mut wire_on = false;
    let mut spin_on = true;
    let mut frames_rendered = 0u32;

    window.render_loop(move |mut frame_input| {
        let mut redraw = frame_input.first_frame;
        redraw |= camera.set_viewport(frame_input.viewport);
        redraw |= control.handle_events(&mut camera, &mut frame_input.events);

        for event in frame_input.events.iter() {
            if let Event::KeyPress { kind, .. } = event {
                match kind {
                    Key::W => {
                        wire_on = !wire_on;
                        redraw = true;
                    }
                    Key::Space => spin_on = !spin_on,
                    Key::Escape => return FrameOutput { exit: true, ..Default::default() },
                    _ => {}
                }
            }
        }

        if spin_on {
            // auto-spin orbits the camera around the model instead of rotating the
            // scene, which keeps arbitrary node transforms intact
            redraw = true;
            let delta = Rad(frame_input.elapsed_time as f32 * 0.00035);
            let target = camera.target();
            let rel = camera.position() - center;
            camera.set_view(
                center + Mat4::from_axis_angle(vec3(0.0, 1.0, 0.0), delta).transform_vector(rel),
                target,
                vec3(0.0, 1.0, 0.0),
            );
        }

        let mut objects: Vec<&dyn Object> = Vec::new();
        models.iter().for_each(|m| objects.push(m));
        if wire_on {
            wireframes.iter().for_each(|w| objects.push(w));
        }

        if redraw {
            frame_input
                .screen()
                .clear(ClearState::color_and_depth(0.10, 0.11, 0.13, 1.0, 1.0))
                .render(&camera, objects.iter().copied(), &[&ambient, &key, &fill]);

            frames_rendered += 1;
            if (selftest && frames_rendered == 5) || (screenshot.is_some() && frames_rendered == 10) {
                // read the frame back from the GPU as RGBA bytes
                let vp = frame_input.viewport;
                let (w, h) = (vp.width as usize, vp.height as usize);
                let px = frame_input.screen().read_color::<[u8; 4]>();

                if let Some(out) = &screenshot {
                    let buf: Vec<u8> = px.iter().flat_map(|c| c.iter()).copied().collect();
                    match image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                        Some(img) => match img.save(out) {
                            Ok(()) => println!("screenshot written to {out}"),
                            Err(e) => eprintln!("failed to write {out}: {e}"),
                        },
                        None => eprintln!("failed to encode screenshot"),
                    }
                    return FrameOutput { exit: true, ..Default::default() };
                }

                // selftest: anything that is not (approximately) the clear color means the model rendered
                let covered = px.iter().filter(|c| c[0].abs_diff(25) > 2).count();
                let pct = covered as f32 / px.len().max(1) as f32 * 100.0;
                if covered == 0 {
                    eprintln!("selftest FAILED: nothing but the clear color on screen");
                } else {
                    println!("selftest PASSED: model covers {pct:.1}% of the viewport");
                }
                return FrameOutput { exit: true, ..Default::default() };
            }
        }

        FrameOutput {
            swap_buffers: redraw,
            ..Default::default()
        }
    });

    Ok(())
}
