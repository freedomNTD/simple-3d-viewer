# simple-3d-viewer

A tiny GLB/glTF/OBJ/STL/3MF viewer written in Rust. Double-click a model, orbit around, done.

<p align="center">
  <img src="docs/screenshot.png" width="80%">
</p>

## Why

Existing Rust glTF viewers are mostly stale experiments; native options on Windows are heavy (Blender) or missing (no 3D Viewer on LTSC). `simple-3d-viewer` is one small binary that opens a GLB and lets you look at it — nothing more.

## Features

- **Instant**: loads and fits the camera to the model automatically, title bar shows vertex/triangle counts
- **Orbit / pan / zoom** with the mouse
- `W` toggle wireframe overlay · `Space` toggle auto-rotate · `Esc` quit
- File dialog when launched without arguments; PBR materials and multi-mesh models supported
- Geometry-only GLBs (e.g. from Hunyuan3D) get a neutral clay material and computed normals, instead of the glTF metallic default that renders near-black without an environment map
- Headless-friendly extras: `--screenshot out.png` renders the model to an image from the GPU framebuffer, `--selftest` verifies the model actually reached the pixels (CI-friendly smoke test)
- Lightweight: plain OpenGL via [three-d](https://github.com/asny/three-d), no runtime dependencies beyond GPU drivers

## Usage

```sh
simple-3d-viewer model.glb                        # open a model
simple-3d-viewer                          # open a file dialog
simple-3d-viewer --screenshot out.png model.glb   # render ~10 frames, save an image, exit
simple-3d-viewer --selftest model.glb             # smoke test: assert the model is visible
simple-3d-viewer --register                       # (Windows) make this the default for .glb/.gltf/.obj/.stl/.3mf
simple-3d-viewer --unregister                     # remove those associations
```

After `--register`, double-clicking a model file in Explorer opens it in the viewer directly.

## Supported formats

| Format | Support |
|--------|---------|
| GLB / glTF | ✅ best (materials, multi-mesh, node transforms) |
| OBJ | ✅ geometry + materials when embedded; external `.mtl`/textures are not resolved |
| STL | ✅ binary & ASCII |
| 3MF | ✅ |
| FBX, DAE, USD(Z), BLEND, 3DS | ❌ not planned for now — these need heavyweight SDKs |

Geometry-only files (e.g. Hunyuan3D GLB exports) get a neutral clay material and computed normals.

Works best with self-contained `.glb` files (e.g. exported from Hunyuan3D, Meshy, Blender). `.gltf` files that reference external `.bin`/textures are not resolved.

## Build

```sh
cargo build --release
# binary at target/release/simple-3d-viewer(.exe)
```

Requires the usual Rust prerequisites (on Windows: MSVC Build Tools).

## 中文简介

用 Rust 写的极简 3D 模型查看器：打开即自动取景，鼠标拖拽旋转/平移/缩放。双击 .glb 文件即可打开，也可拖模型进窗口、修改文件后热重载。`W` 线框、`Space` 自动旋转、`R` 重置相机、`O` 打开其他、`H` 帮助、`Esc` 退出。

## Usage / 使用（Windows）

1. 下载或构建出 `simple-3d-viewer.exe`
2. 双击 exe（不带参数）会弹文件选择框；把模型拖进窗口、或 `simple-3d-viewer model.glb` 直接打开
3. 运行一次 `simple-3d-viewer --register`，之后双击任意 `.glb/.gltf/.obj/.stl/.3mf` 文件即可打开；错误会以弹框提示而不是闪退

## License

MIT
