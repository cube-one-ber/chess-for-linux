#!/usr/bin/env python3
"""Convert the supplied Metal/USD piece models to portable triangle buffers.

Derived assets retain the Apple Sample Code License in the root README.
Requires the OpenUSD Python bindings (`pip install usd-core`) for regeneration
only; the application embeds the resulting buffers and needs no USD runtime.
"""
import math
import struct
from pathlib import Path

from pxr import Gf, Usd, UsdGeom

ROOT = Path(__file__).resolve().parents[1]
SCALE = 0.1  # Original board squares are ten model units wide.


def attribute_value(values, interpolation, face, corner, point):
    index = {
        "constant": 0,
        "uniform": face,
        "vertex": point,
        "varying": point,
        "faceVarying": corner,
    }[interpolation]
    return values[index]


def convert(name):
    stage = Usd.Stage.Open(str(ROOT / "Resources" / "Meshes" / f"{name}.usdc"))
    # Respect the stage axis and hierarchy: the supplied Z-up stages rotate
    # their Y-up mesh coordinates at the root.
    to_y_up = Gf.Matrix4d(1)
    if UsdGeom.GetStageUpAxis(stage) == UsdGeom.Tokens.z:
        to_y_up.SetRotate(Gf.Rotation(Gf.Vec3d(1, 0, 0), -90))
    transforms = UsdGeom.XformCache()
    vertices = []
    for prim in stage.Traverse():
        if not prim.IsA(UsdGeom.Mesh):
            continue
        mesh = UsdGeom.Mesh(prim)
        points = mesh.GetPointsAttr().Get()
        normals = mesh.GetNormalsAttr().Get()
        uv = UsdGeom.PrimvarsAPI(prim).GetPrimvar("UVMap")
        texcoords = uv.ComputeFlattened()
        transform = transforms.GetLocalToWorldTransform(prim) * to_y_up
        normal_transform = transform.GetInverse().GetTranspose()
        indices = mesh.GetFaceVertexIndicesAttr().Get()
        corner = 0
        for face, count in enumerate(mesh.GetFaceVertexCountsAttr().Get()):
            polygon = []
            for offset in range(count):
                point_index = indices[corner + offset]
                position = transform.Transform(Gf.Vec3d(points[point_index])) * SCALE
                normal = attribute_value(
                    normals, mesh.GetNormalsInterpolation(), face,
                    corner + offset, point_index,
                )
                normal = normal_transform.TransformDir(Gf.Vec3d(normal)).GetNormalized()
                texture = attribute_value(
                    texcoords, uv.GetInterpolation(), face, corner + offset, point_index,
                )
                # USD uses bottom-left texture coordinates; decoded images
                # are uploaded to wgpu with their top row first.
                vertex = (*position, *normal, texture[0], 1.0 - texture[1])
                if not all(math.isfinite(value) for value in vertex):
                    raise ValueError(f"{name}: non-finite vertex")
                polygon.append(vertex)
            for offset in range(1, count - 1):
                triangle = [polygon[0], polygon[offset], polygon[offset + 1]]
                if mesh.GetOrientationAttr().Get() == UsdGeom.Tokens.leftHanded:
                    triangle.reverse()
                vertices.extend(triangle)
            corner += count
        if corner != len(indices):
            raise ValueError(f"{name}: incomplete face indices")
    if not vertices:
        raise ValueError(f"{name}: no mesh vertices")
    output = ROOT / "assets" / f"{name}.mesh"
    output.write_bytes(b"".join(struct.pack("<8f", *vertex) for vertex in vertices))
    print(f"{name}: {len(vertices) // 3} triangles")


if __name__ == "__main__":
    for piece in ["pawn", "knight", "bishop", "rook", "queen", "king"]:
        convert(piece)
