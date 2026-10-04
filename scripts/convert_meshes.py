#!/usr/bin/env python3
"""Convert the supplied OpenGL piece geometry to portable triangle buffers.

Derived assets retain the Apple Sample Code License in the root README.
Run from the repository root. Python's standard library is sufficient.
"""
import math
import re
import struct
from pathlib import Path

source = Path('Sources/MBCBoardViewModels.mm').read_text()
scale = 0.085
for name in ['pawn', 'knight', 'bishop', 'rook', 'queen', 'king']:
    body = source.split(f'void draw_{name}(void)')[1].split('\nvoid ')[0]
    radii, heights = [
        [float(v.strip().rstrip('f')) for v in re.search(rf'float trace_{axis}\[\]\s*=\s*\{{(.*?)\}}', body, re.S).group(1).replace('\n', '').split(',') if v.strip()]
        for axis in ['r', 'h']
    ]
    vertices = []
    for j in range(len(radii) - 2):
        r0, r1, h0, h1 = radii[j], radii[j+1], heights[j], heights[j+1]
        dr, dh = r1-r0, h1-h0
        length = math.hypot(dr, dh)
        if not length:
            continue
        for step in range(48):
            quad=[]
            for r,h,a in [(r0,h0,step),(r0,h0,step+1),(r1,h1,step+1),(r1,h1,step)]:
                angle=a*math.tau/48
                quad.append([r*math.sin(angle)*scale,h*scale,r*math.cos(angle)*scale,
                             dh/length*math.sin(angle),-dr/length,dh/length*math.cos(angle),a/48,h/24])
            vertices.extend(quad[k] for k in [0,1,2,0,2,3])
    normal = [0, 1, 0]
    uv = [0, 0]
    mode = None
    group=[]
    # All non-revolved source vertices are literal arithmetic expressions.
    def number(expr):
        expr=expr.strip().replace('kPieceSize','1').replace('kVTex','1').replace('kHTex','1').replace('f','')
        if not re.fullmatch(r'[\d.\s+*/()eE-]+',expr):
            raise ValueError(f'Unexpected geometry expression: {expr}')
        return float(eval(expr, {'__builtins__': {}}, {}))
    for match in re.finditer(r'gl(Begin|End|Normal3f|TexCoord2f|Vertex3f)\((.*?)\)',body):
        op, args=match.groups()
        if op=='Begin':
            mode=args;group=[]
        elif op=='Normal3f': normal=[number(v) for v in args.split(',')]
        elif op=='TexCoord2f': uv=[number(v) for v in args.split(',')]
        elif op=='Vertex3f':
            group.append([number(v)*scale for v in args.split(',')]+normal+uv)
            n=4 if mode=='GL_QUADS' else 3
            if len(group)==n:
                vertices.extend(group[k] for k in ([0,1,2,0,2,3] if n==4 else [0,1,2]))
                group=[]
    output=Path('assets')/f'{name}.mesh'
    output.write_bytes(b''.join(struct.pack('<8f',*v) for v in vertices))
    print(f'{name}: {len(vertices)//3} triangles')
