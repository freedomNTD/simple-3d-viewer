//! Regression guard for the v0.3.0 release: it shipped without the `image`
//! feature on three-d-asset, so *any* GLB with embedded textures failed to
//! parse with "the feature image is needed" — geometry-only files worked,
//! which is exactly why CI's selftest never caught it.
//!
//! This test mirrors the app's parse path (io::load → deserialize into a
//! Model) without needing a GPU, so it runs on every CI platform.

#[test]
fn textured_glb_parses() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/assets/textured.glb");

    let mut loaded = three_d_asset::io::load(&[path]).expect("io::load should succeed");
    let model: three_d_asset::Model = loaded
        .deserialize("textured")
        .expect("embedded texture must decode (three-d-asset needs its `image` feature)");

    assert!(
        model
            .geometries
            .iter()
            .any(|g| matches!(&g.geometry, three_d_asset::Geometry::Triangles(t) if t.triangle_count() > 0)),
        "expected at least one triangle mesh"
    );
    assert!(
        model.materials.iter().any(|m| m.albedo_texture.is_some()),
        "the material should carry a decoded albedo texture"
    );
}
