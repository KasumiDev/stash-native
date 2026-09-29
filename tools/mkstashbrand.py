#!/usr/bin/env python3
"""Generate StashNative's simple typography masters using the bundled licensed font."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
BG = "#060606"
INK = "#f4f4f4"
font = ROOT / "pkg/appfont-bold.ttf"

def centered(draw, text, center, size):
    face = ImageFont.truetype(str(font), size)
    box = draw.textbbox((0, 0), text, font=face)
    draw.text((center[0] - (box[2] + box[0]) / 2,
               center[1] - (box[3] + box[1]) / 2), text, font=face, fill=INK)

logo = Image.new("RGB", (1024, 1024), BG)
draw = ImageDraw.Draw(logo)
draw.rounded_rectangle((185, 185, 839, 839), radius=145, outline=INK, width=28)
centered(draw, "S", (512, 500), 570)
logo.save(ROOT / "assets/logo-master.png")
splash = Image.new("RGB", (1920, 1080), BG)
draw = ImageDraw.Draw(splash)
centered(draw, "StashNative", (960, 540), 110)
splash.save(ROOT / "assets/splash-master.png")
