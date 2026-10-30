"""Gera o ícone do GuitarForge (palheta âmbar com um knob de amp) sem biblioteca de imagem.

Desenho por campo de distância com antialiasing; saída em assets/ do app: guitarforge.png (256 px, ícone da
janela) e guitarforge.ico (16 a 256 px em PNG, embutido no .exe pelo build.rs).
Uso: python scripts/gerar_icone.py
"""
import math
import os
import struct
import zlib

BG = (0x1B, 0x1E, 0x24)
BG_RIM = (0x34, 0x39, 0x43)
AMBER_TOP = (0xFF, 0xC2, 0x66)
AMBER_BOTTOM = (0xF0, 0x93, 0x2B)
KNOB = (0x1B, 0x1E, 0x24)
KNOB_RIM = (0x4A, 0x50, 0x5A)
POINTER = (0xFF, 0xB3, 0x47)


def sd_round_box(px, py, cx, cy, half, r):
    qx = abs(px - cx) - half + r
    qy = abs(py - cy) - half + r
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - r


def sd_triangle(px, py, a, b, c):
    def sub(u, v):
        return (u[0] - v[0], u[1] - v[1])

    def dot(u, v):
        return u[0] * v[0] + u[1] * v[1]

    p = (px, py)
    e0, e1, e2 = sub(b, a), sub(c, b), sub(a, c)
    v0, v1, v2 = sub(p, a), sub(p, b), sub(p, c)

    def proj(v, e):
        t = max(0.0, min(1.0, dot(v, e) / dot(e, e)))
        return (v[0] - e[0] * t, v[1] - e[1] * t)

    pq0, pq1, pq2 = proj(v0, e0), proj(v1, e1), proj(v2, e2)
    s = 1.0 if e0[0] * e2[1] - e0[1] * e2[0] > 0 else -1.0
    # sdTriangle de Inigo Quilez: mínimo de cada componente em separado
    dist2 = min(dot(pq0, pq0), dot(pq1, pq1), dot(pq2, pq2))
    side = min(
        s * (v0[0] * e0[1] - v0[1] * e0[0]),
        s * (v1[0] * e1[1] - v1[1] * e1[0]),
        s * (v2[0] * e2[1] - v2[1] * e2[0]),
    )
    return -math.sqrt(dist2) * math.copysign(1.0, side)


def sd_capsule(px, py, ax, ay, bx, by, r):
    pax, pay = px - ax, py - ay
    bax, bay = bx - ax, by - ay
    h = max(0.0, min(1.0, (pax * bax + pay * bay) / (bax * bax + bay * bay)))
    return math.hypot(pax - bax * h, pay - bay * h) - r


def mix(a, b, t):
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))


def over(dst, color, alpha):
    r, g, b, a = dst
    out_a = alpha + a * (1 - alpha)
    if out_a <= 0:
        return (0.0, 0.0, 0.0, 0.0)
    blend = [(color[i] * alpha + (r, g, b)[i] * a * (1 - alpha)) / out_a for i in range(3)]
    return (blend[0], blend[1], blend[2], out_a)


def render(size):
    k = size / 256.0
    angle = math.radians(-50.0)
    knob_c = (128.0, 110.0)
    tip = (knob_c[0] + math.cos(angle) * 30.0, knob_c[1] + math.sin(angle) * 30.0)
    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            px, py = (x + 0.5) / k, (y + 0.5) / k
            cov = lambda d: max(0.0, min(1.0, 0.5 - d * k))
            pix = (0.0, 0.0, 0.0, 0.0)
            tile = sd_round_box(px, py, 128.0, 128.0, 122.0, 54.0)
            pix = over(pix, BG_RIM, cov(tile))
            pix = over(pix, BG, cov(tile + 4.0))
            pick = sd_triangle(px, py, (58.0, 64.0), (198.0, 64.0), (128.0, 200.0)) - 30.0
            pix = over(pix, mix(AMBER_TOP, AMBER_BOTTOM, max(0.0, min(1.0, (py - 34.0) / 196.0))), cov(pick))
            knob = math.hypot(px - knob_c[0], py - knob_c[1]) - 42.0
            pix = over(pix, KNOB_RIM, cov(knob))
            pix = over(pix, KNOB, cov(knob + 5.0))
            pointer = sd_capsule(px, py, knob_c[0], knob_c[1], tip[0], tip[1], 5.5)
            pix = over(pix, POINTER, cov(pointer))
            r, g, b, a = pix
            row += bytes((round(r), round(g), round(b), round(a * 255)))
        rows.append(bytes(row))
    return rows


def png(size, rows):
    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")


def ico(images):
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for size, data in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        blobs += data
    return header + entries + blobs


def main():
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    # repositório privado: o app fica em amp/; no público o app é a raiz
    out = os.path.join(root, "amp", "assets") if os.path.isdir(os.path.join(root, "amp")) else os.path.join(root, "assets")
    os.makedirs(out, exist_ok=True)
    images = []
    for size in (256, 128, 64, 48, 32, 24, 16):
        images.append((size, png(size, render(size))))
    with open(os.path.join(out, "guitarforge.png"), "wb") as f:
        f.write(images[0][1])
    with open(os.path.join(out, "guitarforge.ico"), "wb") as f:
        f.write(ico(images))
    print("ok:", os.path.normpath(out))


if __name__ == "__main__":
    main()
