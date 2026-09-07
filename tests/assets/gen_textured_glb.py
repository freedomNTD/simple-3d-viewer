"""Generate tests/assets/textured.glb — a minimal self-contained GLB with an
embedded PNG texture (textured quad, 2 triangles). ~1KB. Hand-built without
external deps so the asset can be regenerated deterministically:

    python tests/assets/gen_textured_glb.py > tests/assets/textured.glb
"""
import struct
import sys
import zlib


def png_chunk(typ: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data)) + typ + data
        + struct.pack(">I", zlib.crc32(typ + data) & 0xFFFFFFFF)
    )


def make_png() -> bytes:
    # 2x2 RGBA, four distinct colors so texture application is visible
    ihdr = struct.pack(">IIBBBBB", 2, 2, 8, 6, 0, 0, 0)
    row = lambda px: b"\x00" + b"".join(bytes(p) for p in px)
    raw = row([(255, 60, 60, 255), (60, 255, 60, 255)]) + row(
        [(60, 60, 255, 255), (255, 220, 40, 255)]
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", zlib.compress(raw))
        + png_chunk(b"IEND", b"")
    )


def main() -> None:
    png = make_png()

    positions = [(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0)]
    normals = [(0, 0, 1)] * 4
    uvs = [(0, 0), (1, 0), (1, 1), (0, 1)]
    indices = [0, 1, 2, 0, 2, 3]

    pos_bytes = struct.pack("<12f", *(c for v in positions for c in v))
    nrm_bytes = struct.pack("<12f", *(c for v in normals for c in v))
    uv_bytes = struct.pack("<8f", *(c for v in uvs for c in v))
    idx_bytes = struct.pack("<6H", *indices)

    png_off = 128
    png_pad = (-png_off - len(png)) % 4
    idx_off = png_off + len(png) + png_pad
    bin_data = (
        pos_bytes + nrm_bytes + uv_bytes
        + png + b"\x00" * png_pad + idx_bytes
    )

    gltf = {
        "asset": {"version": "2.0", "generator": "simple-3d-viewer test asset"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{
            "primitives": [{
                "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
                "indices": 3,
                "material": 0,
            }]
        }],
        "materials": [{
            "pbrMetallicRoughness": {
                "baseColorTexture": {"index": 0},
                "metallicFactor": 0.0,
                "roughnessFactor": 0.8,
            }
        }],
        "textures": [{"sampler": 0, "source": 0}],
        "samplers": [{"magFilter": 9729, "minFilter": 9729, "wrapS": 33071, "wrapT": 33071}],
        "images": [{"bufferView": 3, "mimeType": "image/png"}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(pos_bytes), "target": 34962},
            {"buffer": 0, "byteOffset": 48, "byteLength": len(nrm_bytes), "target": 34962},
            {"buffer": 0, "byteOffset": 96, "byteLength": len(uv_bytes), "target": 34962},
            {"buffer": 0, "byteOffset": png_off, "byteLength": len(png)},
            {"buffer": 0, "byteOffset": idx_off, "byteLength": len(idx_bytes), "target": 34963},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
             "min": [-1, -1, 0], "max": [1, 1, 0]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2"},
            {"bufferView": 4, "componentType": 5123, "count": 6, "type": "SCALAR"},
        ],
        "buffers": [{"byteLength": len(bin_data)}],
    }
    import json
    json_data = json.dumps(gltf, separators=(",", ":")).encode()
    json_pad = b" " * ((-len(json_data)) % 4)
    bin_pad = b"\x00" * ((-len(bin_data)) % 4)

    out = b"glTF"
    out += struct.pack("<II", 2, 12 + 8 + len(json_data) + len(json_pad) + 8 + len(bin_data) + len(bin_pad))
    out += struct.pack("<I", len(json_data) + len(json_pad)) + b"JSON" + json_data + json_pad
    out += struct.pack("<I", len(bin_data) + len(bin_pad)) + b"BIN\x00" + bin_data + bin_pad
    sys.stdout.buffer.write(out)


if __name__ == "__main__":
    main()
