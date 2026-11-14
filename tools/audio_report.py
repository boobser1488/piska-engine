#!/usr/bin/env python3
"""Анализ звука движка без прослушивания.

Читает WAV (PCM 16/24/32 или float32), которые рендерит
`cargo run -p engine-audio --example audio_render -- <dir>`, и считает для каждого файла:

* peak, true peak (4x передискретизация), RMS, громкость LUFS-ish (K-взвешивание
  BS.1770 в частотной области + стробирование -70 / -10 LU), crest factor;
* число «упёршихся» сэмплов (|x| >= 0.999), постоянную составляющую (DC);
* долю энергии выше 8 / 12 / 16 кГц;
* для осцилляторов `osc/*` — негармоническую энергию (оценка наложения спектров);
* скачки сэмпл-к-сэмплу (щелчки) и скачок на стыке петли (для `music/*`).

Использование:

    python tools/audio_report.py <dir>                    # таблица + <dir>/report.json
    python tools/audio_report.py <dir> --spectrograms     # ещё PNG-спектрограммы в <dir>/png
    python tools/audio_report.py --compare <before> <after>

Нужны только numpy и Pillow (Pillow — для спектрограмм). Ничего не воспроизводит.
"""

import json
import math
import struct
import sys
from pathlib import Path

import numpy as np


# ---------------------------------------------------------------------------
# Чтение WAV
# ---------------------------------------------------------------------------

def read_wav(path):
    data = Path(path).read_bytes()
    if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError(f"{path}: not a RIFF/WAVE file")
    pos = 12
    fmt = None
    pcm = None
    while pos + 8 <= len(data):
        cid = data[pos:pos + 4]
        size = struct.unpack("<I", data[pos + 4:pos + 8])[0]
        body = data[pos + 8:pos + 8 + size]
        if cid == b"fmt ":
            tag, ch, rate, _, _, bits = struct.unpack("<HHIIHH", body[:16])
            if tag == 0xFFFE and len(body) >= 26:
                tag = struct.unpack("<H", body[24:26])[0]
            fmt = (tag, ch, rate, bits)
        elif cid == b"data":
            pcm = body
        pos += 8 + size + (size & 1)
    if fmt is None or pcm is None:
        raise ValueError(f"{path}: missing fmt or data chunk")
    tag, ch, rate, bits = fmt
    if tag == 3 and bits == 32:
        x = np.frombuffer(pcm, dtype="<f4").astype(np.float64)
    elif tag == 1 and bits == 16:
        x = np.frombuffer(pcm, dtype="<i2").astype(np.float64) / 32768.0
    elif tag == 1 and bits == 32:
        x = np.frombuffer(pcm, dtype="<i4").astype(np.float64) / 2147483648.0
    elif tag == 1 and bits == 24:
        b = np.frombuffer(pcm, dtype=np.uint8).reshape(-1, 3).astype(np.int32)
        v = b[:, 0] | (b[:, 1] << 8) | (b[:, 2] << 16)
        v = np.where(v >= 1 << 23, v - (1 << 24), v)
        x = v.astype(np.float64) / float(1 << 23)
    else:
        raise ValueError(f"{path}: unsupported format tag={tag} bits={bits}")
    x = x[: len(x) // ch * ch].reshape(-1, ch)
    return rate, x


# ---------------------------------------------------------------------------
# Метрики
# ---------------------------------------------------------------------------

def db(x, floor=-200.0):
    return float(20.0 * math.log10(x)) if x > 0 else floor


def fft_convolve(x, h):
    n = len(x) + len(h) - 1
    size = 1 << (n - 1).bit_length()
    return np.fft.irfft(np.fft.rfft(x, size) * np.fft.rfft(h, size), size)[:n]


def true_peak(x):
    """Пик после 4x передискретизации (оконный sinc, 48 отводов на фазу)."""
    taps = 48 * 4
    t = (np.arange(taps) - (taps - 1) / 2) / 4.0
    h = np.sinc(t) * np.kaiser(taps, 8.0)
    peak = 0.0
    for c in range(x.shape[1]):
        up = np.zeros(len(x) * 4)
        up[::4] = x[:, c]
        y = fft_convolve(up, h)
        peak = max(peak, float(np.max(np.abs(y))))
    return peak


def k_weight_mag(freqs, rate):
    """|H(f)| K-фильтра BS.1770 (полка +4 дБ у 1.5 кГц и ФВЧ 38 Гц) по аналоговому прототипу,
    пересчитанному через билинейное преобразование для частоты `rate`."""
    def biquad_mag(b, a):
        w = 2 * np.pi * freqs / rate
        z = np.exp(-1j * w)
        num = b[0] + b[1] * z + b[2] * z * z
        den = a[0] + a[1] * z + a[2] * z * z
        return np.abs(num / den)

    # Полка и ФВЧ по формулам BS.1770 (как у pyloudnorm, через K = tan(pi*fc/fs)).
    G, Q, fc = 3.999843853973347, 0.7071752369554196, 1681.974450955533
    K = np.tan(np.pi * fc / rate)
    Vh = 10 ** (G / 20.0)
    Vb = Vh ** 0.4996667741545416
    a0 = 1 + K / Q + K * K
    b = [(Vh + Vb * K / Q + K * K) / a0, 2 * (K * K - Vh) / a0, (Vh - Vb * K / Q + K * K) / a0]
    a = [1.0, 2 * (K * K - 1) / a0, (1 - K / Q + K * K) / a0]
    shelf = biquad_mag(b, a)
    fc, Q = 38.13547087602444, 0.5003270373238773
    K = np.tan(np.pi * fc / rate)
    a0 = 1 + K / Q + K * K
    b = [1.0, -2.0, 1.0]
    a = [1.0, 2 * (K * K - 1) / a0, (1 - K / Q + K * K) / a0]
    hp = biquad_mag(b, a)
    return shelf * hp


def lufs(x, rate):
    """Интегральная громкость, LUFS-ish: K-взвешивание с нулевой фазой (в частотной
    области), блоки 400 мс с перекрытием 75 %, стробирование -70 LUFS и -10 LU."""
    n = len(x)
    if n < int(0.4 * rate):
        x = np.vstack([x, np.zeros((int(0.4 * rate) - n, x.shape[1]))])
        n = len(x)
    size = 1 << (n - 1).bit_length()
    freqs = np.fft.rfftfreq(size, 1.0 / rate)
    mag = k_weight_mag(freqs, rate)
    power = np.zeros(n)
    for c in range(x.shape[1]):
        y = np.fft.irfft(np.fft.rfft(x[:, c], size) * mag, size)[:n]
        power += y * y
    block = int(0.4 * rate)
    hop = block // 4
    starts = range(0, n - block + 1, hop)
    z = np.array([power[s:s + block].mean() for s in starts])
    if len(z) == 0:
        return -200.0
    loud = -0.691 + 10 * np.log10(np.maximum(z, 1e-20))
    gated = z[loud > -70.0]
    if len(gated) == 0:
        return -200.0
    rel = -0.691 + 10 * np.log10(gated.mean()) - 10.0
    gated = z[(loud > -70.0) & (loud > rel)]
    return float(-0.691 + 10 * np.log10(gated.mean()))


def band_energy(mono, rate):
    """Доля энергии выше 8/12/16 кГц, дБ относительно всей энергии."""
    spec = np.abs(np.fft.rfft(mono * np.hanning(len(mono)))) ** 2
    freqs = np.fft.rfftfreq(len(mono), 1.0 / rate)
    total = spec.sum() + 1e-30
    out = {}
    for f in (8000, 12000, 16000):
        out[f"above_{f // 1000}k_db"] = round(10 * math.log10(spec[freqs >= f].sum() / total + 1e-20), 1)
    return out


def aliasing(mono, rate, f0):
    """Негармоническая энергия чистого осциллятора: всё, что не у k·f0, дБ к общей."""
    n = len(mono)
    w = np.blackman(n)
    spec = np.abs(np.fft.rfft(mono * w)) ** 2
    freqs = np.fft.rfftfreq(n, 1.0 / rate)
    df = rate / n
    mask = np.zeros(len(spec), dtype=bool)
    mask[freqs < 20] = True
    k = 1
    while k * f0 < rate / 2:
        mask |= np.abs(freqs - k * f0) <= 6 * df
        k += 1
    total = spec.sum() + 1e-30
    inh = spec[~mask].sum()
    # Худшая одиночная негармоническая линия.
    worst = spec[~mask].max() if (~mask).any() else 0.0
    return round(10 * math.log10(inh / total + 1e-20), 1), round(10 * math.log10(worst / spec.max() + 1e-20), 1)


def steps(x):
    d = np.abs(np.diff(x, axis=0)).max(axis=1) if len(x) > 1 else np.zeros(1)
    return float(d.max()), int((d > 0.1).sum()), int((d > 0.25).sum())


def analyze(path):
    rate, x = read_wav(path)
    mono = x.mean(axis=1)
    peak = float(np.max(np.abs(x))) if x.size else 0.0
    rms = float(np.sqrt(np.mean(x ** 2))) if x.size else 0.0
    loud = lufs(x, rate)
    max_step, steps01, steps025 = steps(x)
    r = {
        "file": str(path),
        "rate": rate,
        "channels": x.shape[1],
        "seconds": round(len(x) / rate, 2),
        "peak_dbfs": round(db(peak), 2),
        "true_peak_dbtp": round(db(true_peak(x)), 2),
        "rms_dbfs": round(db(rms), 2),
        "lufs": round(loud, 1),
        "crest_db": round(db(peak) - db(rms), 1) if rms > 0 else 0.0,
        "clipped": int((np.abs(x) >= 0.999).sum()),
        "over_1": int((np.abs(x) > 1.0).sum()),
        "dc": round(float(np.mean(mono)), 5),
        "max_step": round(max_step, 3),
        "steps_gt_0.1": steps01,
        "steps_gt_0.25": steps025,
    }
    r.update(band_energy(mono, rate))
    if "/music/" in str(path).replace("\\", "/"):
        # Скачок на стыке петли против типичного шага внутри.
        jump = float(np.abs(x[0] - x[-1]).max())
        typical = float(np.percentile(np.abs(np.diff(mono)), 99))
        r["loop_jump"] = round(jump, 4)
        r["loop_jump_vs_p99_step"] = round(jump / (typical + 1e-9), 1)
        r["start_abs"] = round(float(np.abs(x[0]).max()), 4)
    name = Path(path).stem
    if name.endswith("_3520"):
        r["inharmonic_db"], r["worst_alias_line_db"] = aliasing(mono, rate, 3520.0)
    if name.startswith("resample_"):
        # Синус 9 кГц × 1.5 = 13.5 кГц: всё остальное — продукт ресемплера.
        r["inharmonic_db"], r["worst_alias_line_db"] = aliasing_single(mono, rate, 13500.0)
    return r


def aliasing_single(mono, rate, f):
    # Только установившийся участок (без начала и хвоста).
    seg = mono[int(0.05 * rate): int(0.05 * rate) + 16384]
    w = np.blackman(len(seg))
    spec = np.abs(np.fft.rfft(seg * w)) ** 2
    freqs = np.fft.rfftfreq(len(seg), 1.0 / rate)
    mask = np.abs(freqs - f) <= 8 * rate / len(seg)
    total = spec.sum() + 1e-30
    inh = spec[~mask].sum()
    return round(10 * math.log10(inh / total + 1e-20), 1), round(10 * math.log10(spec[~mask].max() / spec.max() + 1e-20), 1)


# ---------------------------------------------------------------------------
# Спектрограммы (numpy + Pillow)
# ---------------------------------------------------------------------------

def colormap(t):
    """Тёмно-синий → фиолетовый → оранжевый → светло-жёлтый (похоже на «inferno»)."""
    stops = np.array([
        [0.0, 0, 0, 4], [0.25, 60, 15, 110], [0.5, 180, 50, 90],
        [0.75, 245, 140, 30], [1.0, 252, 255, 164]], dtype=np.float64)
    t = np.clip(t, 0, 1)
    out = np.zeros(t.shape + (3,))
    for c in range(3):
        out[..., c] = np.interp(t, stops[:, 0], stops[:, c + 1])
    return out.astype(np.uint8)


def spectrogram(path, png, width=1200, height=400, floor=-110.0):
    from PIL import Image, ImageDraw

    rate, x = read_wav(path)
    mono = x.mean(axis=1)
    n_fft, hop = 2048, 256
    if len(mono) < n_fft:
        mono = np.pad(mono, (0, n_fft - len(mono)))
    frames = 1 + (len(mono) - n_fft) // hop
    win = np.hanning(n_fft)
    idx = np.arange(n_fft)[None, :] + hop * np.arange(frames)[:, None]
    spec = np.abs(np.fft.rfft(mono[idx] * win, axis=1)) / (win.sum() / 2)
    sdb = 20 * np.log10(spec + 1e-12)
    # Сжать по времени до ширины картинки (максимум — чтобы щелчки не терялись).
    if frames > width:
        edges = np.linspace(0, frames, width + 1).astype(int)
        sdb = np.array([sdb[a:max(b, a + 1)].max(axis=0) for a, b in zip(edges[:-1], edges[1:])])
    img = colormap((sdb.T[::-1] - floor) / -floor)
    im = Image.fromarray(img).resize((width, height), Image.NEAREST)
    # Полоса волны сверху: красным — сэмплы у полной шкалы.
    wave_h = 80
    canvas = Image.new("RGB", (width, height + wave_h + 18), (16, 16, 20))
    canvas.paste(im, (0, wave_h + 18))
    d = ImageDraw.Draw(canvas)
    edges = np.linspace(0, len(x), width + 1).astype(int)
    for px in range(width):
        seg = x[edges[px]:max(edges[px + 1], edges[px] + 1)]
        if len(seg) == 0:
            continue
        hi, lo = float(seg.max()), float(seg.min())
        mid = 18 + wave_h / 2
        col = (230, 60, 60) if max(abs(hi), abs(lo)) >= 0.999 else (120, 190, 255)
        d.line([(px, mid - hi * wave_h / 2), (px, mid - lo * wave_h / 2)], fill=col)
    for khz in (4, 8, 12, 16, 20):
        if khz * 1000 < rate / 2:
            y = wave_h + 18 + height - int(height * khz * 1000 / (rate / 2))
            d.line([(0, y), (6, y)], fill=(255, 255, 255))
            d.text((8, y - 6), f"{khz}k", fill=(255, 255, 255))
    d.text((4, 2), f"{Path(path).name}  {rate} Hz  {len(x) / rate:.1f} s  (dB floor {floor:.0f})", fill=(230, 230, 230))
    canvas.save(png)


# ---------------------------------------------------------------------------
# Вывод
# ---------------------------------------------------------------------------

COLS = ["peak_dbfs", "true_peak_dbtp", "rms_dbfs", "lufs", "crest_db", "clipped", "dc",
        "above_8k_db", "above_12k_db", "above_16k_db", "max_step", "steps_gt_0.1"]


def table(results, cols=COLS):
    head = "| file | " + " | ".join(cols) + " |"
    sep = "|---|" + "---|" * len(cols)
    rows = [head, sep]
    for r in results:
        rows.append("| " + Path(r["file"]).parent.name + "/" + Path(r["file"]).stem + " | "
                    + " | ".join(str(r.get(c, "")) for c in cols) + " |")
    return "\n".join(rows)


def run_dir(root, spectro):
    root = Path(root)
    files = sorted(root.glob("*/*.wav"))
    results = [analyze(f) for f in files]
    (root / "report.json").write_text(json.dumps(results, indent=1))
    print(table(results))
    extra = [r for r in results if "loop_jump" in r or "inharmonic_db" in r]
    if extra:
        print()
        print(table(extra, ["loop_jump", "loop_jump_vs_p99_step", "start_abs", "inharmonic_db", "worst_alias_line_db"]))
    perf = root / "perf.txt"
    if perf.exists():
        print()
        print(perf.read_text())
    if spectro:
        out = root / "png"
        out.mkdir(exist_ok=True)
        for f in files:
            if f.parent.name in ("mix", "osc") or f.stem in ("swarm_game", "swarm_boss", "lantern_deep", "marble_factory_b", "engine_adventure"):
                spectrogram(f, out / (f.parent.name + "_" + f.stem + ".png"))
    return results


def compare(before, after):
    a = {Path(r["file"]).parent.name + "/" + Path(r["file"]).stem: r for r in json.loads((Path(before) / "report.json").read_text())}
    b = {Path(r["file"]).parent.name + "/" + Path(r["file"]).stem: r for r in json.loads((Path(after) / "report.json").read_text())}
    cols = ["peak_dbfs", "true_peak_dbtp", "lufs", "crest_db", "clipped", "above_12k_db", "max_step", "steps_gt_0.1",
            "loop_jump", "inharmonic_db"]
    print("| file | " + " | ".join(cols) + " |")
    print("|---|" + "---|" * len(cols))
    for k in sorted(set(a) & set(b)):
        cells = []
        for c in cols:
            va, vb = a[k].get(c, ""), b[k].get(c, "")
            cells.append("" if va == "" and vb == "" else f"{va} -> {vb}")
        print(f"| {k} | " + " | ".join(cells) + " |")


def main(argv):
    if len(argv) >= 3 and argv[0] == "--compare":
        compare(argv[1], argv[2])
        return 0
    if not argv:
        print(__doc__)
        return 1
    run_dir(argv[0], "--spectrograms" in argv)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
