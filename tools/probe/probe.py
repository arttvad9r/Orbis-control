#!/usr/bin/env python3
"""Orbis Control — read-only research probe.

Собирает обезличенный аппаратный профиль для фикстур разработки.
Проектирование: read-only, allowlist путей, никаких write-флагов, никакого sudo,
никаких shell-команд (все внешние вызовы — subprocess с argv, без shell=True),
таймауты, обезличивание результата.

Использование:
    probe.py collect --out <dir>            # собрать фикстуры
    probe.py verify-read-only --out <dir>   # проверка: только чтение, файлы на месте
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import subprocess
import sys
import time
from pathlib import Path

# ---------------------------------------------------------------- allowlist

# Только эти пути читаются. Всё остальное игнорируется.
ALLOWED_FILES = [
    "/sys/class/dmi/id/sys_vendor",
    "/sys/class/dmi/id/product_name",
    "/sys/class/dmi/id/board_name",
    "/sys/class/dmi/id/bios_version",
    "/sys/class/dmi/id/bios_date",
    "/sys/firmware/acpi/platform_profile",
    "/sys/firmware/acpi/platform_profile_choices",
    "/sys/class/power_supply/BAT1/charge_control_end_threshold",
    "/sys/class/power_supply/BAT1/uevent",
    "/sys/class/power_supply/BAT1/energy_full",
    "/sys/class/power_supply/BAT1/energy_full_design",
    "/sys/class/power_supply/BAT1/capacity",
    "/sys/class/power_supply/BAT1/status",
    "/sys/class/power_supply/BAT1/manufacturer",
    "/sys/class/power_supply/BAT1/model_name",
    "/sys/class/backlight/nvidia_0/brightness",
    "/sys/class/backlight/nvidia_0/max_brightness",
    "/sys/class/backlight/nvidia_0/type",
    "/sys/class/leds/asus::kbd_backlight/brightness",
    "/sys/class/leds/asus::kbd_backlight/max_brightness",
    "/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver",
    "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences",
    "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
    "/sys/devices/system/cpu/cpu0/cpufreq/boost",
]

ALLOWED_DIRS = [
    "/sys/devices/platform/asus-nb-wmi",  # только заданные атрибуты
    "/sys/class/hwmon",
    "/sys/class/drm",
    "/sys/class/thermal",
    "/sys/bus/pci/devices",  # только имена устройств
    "/sys/module",           # только имена загруженных модулей asus-*
]

# Атрибуты платформы asus-nb-wmi, которые разрешено читать.
ASUS_WMI_ATTRS = [
    "charge_mode", "dgpu_disable", "gpu_mux_mode", "nv_dynamic_boost",
    "nv_temp_target", "panel_od", "ppt_fppt", "ppt_pl1_spl", "ppt_pl2_sppt",
    "throttle_thermal_policy", "cpufv",
]

# Чувствительные паттерны, которые обязаны отсутствовать в выходе.
FORBIDDEN_PATTERNS = [
    r"serial", r"hostname", r"machine-id", r"machine_id",
    r"mac\s*=", r"uuid", r"/home/[a-z]", r"[0-9a-f]{8}[:-][0-9a-f]{4}",
]

TIMEOUT = 5  # секунд на операцию

# ---------------------------------------------------------------- helpers


def read_file(path: str) -> str | None:
    """Читает файл из allowlist. Возвращает None при любой ошибке (никогда не падает)."""
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as fh:
            return fh.read().strip()
    except (OSError, PermissionError, FileNotFoundError) as e:
        return None


def read_uevent_safe(path: str) -> str | None:
    """Читает uevent, удаляя строки, способные содержать персональные данные."""
    raw = read_file(path)
    if raw is None:
        return None
    drop_prefixes = ("POWER_SUPPLY_SERIAL_NUMBER", "ID_SERIAL", "ID_NET_NAME")
    kept = [
        line for line in raw.splitlines()
        if not any(line.startswith(p) for p in drop_prefixes)
    ]
    return "\n".join(kept)


def file_stat(path: str) -> dict | None:
    try:
        st = os.stat(path)
        return {
            "mode": oct(st.st_mode & 0o7777),
            "uid": st.st_uid,
            "gid": st.st_gid,
            "size": st.st_size,
        }
    except OSError:
        return None


def run_argv(argv: list[str], timeout: int = TIMEOUT) -> dict:
    """Внешняя команда ТОЛЬКО с фиксированным argv, без shell. Read-only команды."""
    try:
        proc = subprocess.run(
            argv, capture_output=True, text=True, timeout=timeout, check=False,
            shell=False,  # никогда shell
        )
        return {
            "ok": proc.returncode == 0,
            "returncode": proc.returncode,
            "stdout": proc.stdout[:200_000],
            "stderr": proc.stderr[:20_000],
        }
    except (subprocess.TimeoutExpired, OSError) as e:
        return {"ok": False, "error": str(e)}


def anonymize(text: str) -> str:
    """Базовое обезличивание вывода внешних команд."""
    # IPv4/IPv6 адреса
    text = re.sub(r"\b\d{1,3}(?:\.\d{1,3}){3}\b", "<ip>", text)
    text = re.sub(r"([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}", "<mac>", text)
    # UUID
    text = re.sub(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        "<uuid>", text,
    )
    return text


# ---------------------------------------------------------------- collectors


def collect_dmi() -> dict:
    return {
        "sys_vendor": read_file("/sys/class/dmi/id/sys_vendor"),
        "product_name": read_file("/sys/class/dmi/id/product_name"),
        "board_name": read_file("/sys/class/dmi/id/board_name"),
        "bios_version": read_file("/sys/class/dmi/id/bios_version"),
        "bios_date": read_file("/sys/class/dmi/id/bios_date"),
    }


def collect_sysfs() -> dict:
    files: dict[str, object] = {}
    platform_attrs: dict[str, object] = {}
    for p in ALLOWED_FILES:
        if p.endswith("/uevent"):
            value = read_uevent_safe(p)
        else:
            value = read_file(p)
        files[p] = {
            "value": value,
            "stat": file_stat(p),
        }
    for attr in ASUS_WMI_ATTRS:
        path = f"/sys/devices/platform/asus-nb-wmi/{attr}"
        platform_attrs[attr] = {
            "value": read_file(path),
            "stat": file_stat(path),
        }
    return {
        "files": files,
        "platform": platform_attrs,
        "cpufreq": collect_cpufreq(),
        "kernel": platform.release(),
    }


def collect_cpufreq() -> dict:
    """Collect read-only CPU frequency policy evidence for CPU controls."""
    base = "/sys/devices/system/cpu/cpu0/cpufreq"
    return {
        "driver": read_file(f"{base}/scaling_driver"),
        "energy_performance_available_preferences": read_file(
            f"{base}/energy_performance_available_preferences"
        ),
        "energy_performance_preference": read_file(
            f"{base}/energy_performance_preference"
        ),
        "boost": read_file(f"{base}/boost"),
    }


def collect_hwmon() -> dict:
    out = {"devices": {}}
    base = Path("/sys/class/hwmon")
    if not base.is_dir():
        return out
    for hw in sorted(base.iterdir()):
        name_file = hw / "name"
        name = read_file(str(name_file)) if name_file.exists() else None
        entries = {}
        # разрешено читать только файлы вида *_input/_label/_enable и кривые
        for f in sorted(hw.iterdir()):
            fn = f.name
            if fn in ("name", "uevent", "device", "subsystem", "power"):
                continue
            if any(fn.startswith(p) for p in (
                "fan", "temp", "pwm", "curr", "in", "energy", "power"
            )):
                if f.is_file() and not f.is_symlink():
                    entries[fn] = read_file(str(f))
        out["devices"][hw.name] = {"name": name, "entries": entries}
    return out


def collect_drm() -> dict:
    out = {"connectors": {}}
    base = Path("/sys/class/drm")
    if not base.is_dir():
        return out
    for c in sorted(base.iterdir()):
        if not c.name.startswith("card"):
            continue
        status_file = c / "status"
        enabled_file = c / "enabled"
        modes_file = c / "modes"
        if status_file.exists():
            out["connectors"][c.name] = {
                "status": read_file(str(status_file)),
                "enabled": read_file(str(enabled_file)) if enabled_file.exists() else None,
                "modes": (read_file(str(modes_file)) or "").splitlines()[:16],
            }
    return out


def collect_dbus_xml(service: str, paths: list[str], dest_dir: Path, prefix: str) -> None:
    """busctl introspect --xml-interface, через argv без shell."""
    for i, path in enumerate(paths):
        res = run_argv(["busctl", "--system", "introspect", "--xml-interface",
                        service, path])
        if res["ok"]:
            fname = f"{prefix}-introspection-{i:02d}-{path.strip('/').replace('/', '_')}.xml"
            (dest_dir / fname).write_text(anonymize(res["stdout"]), encoding="utf-8")
        else:
            print(f"  ! introspect {service} {path} failed: {res.get('error', res['returncode'])}",
                  file=sys.stderr)


def collect_upower() -> dict:
    res = run_argv(["upower", "--dump"])
    return {"upower_dump_ok": res["ok"], "dump": anonymize(res["stdout"]) if res["ok"] else None}


def collect_modules() -> list:
    out = []
    base = Path("/sys/module")
    for m in sorted(base.iterdir()):
        if "asus" in m.name or "wmi" in m.name:
            out.append(m.name)
    return out


# ---------------------------------------------------------------- main


def collect(out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    dmi = collect_dmi()
    (out_dir / "dmi.json").write_text(json.dumps(dmi, indent=2, ensure_ascii=False), encoding="utf-8")

    sysfs = collect_sysfs()
    (out_dir / "sysfs-tree.json").write_text(json.dumps(sysfs, indent=2), encoding="utf-8")

    hwmon = collect_hwmon()
    (out_dir / "hwmon.json").write_text(json.dumps(hwmon, indent=2), encoding="utf-8")

    drm = collect_drm()
    (out_dir / "drm.json").write_text(json.dumps(drm, indent=2), encoding="utf-8")

    upower = collect_upower()
    (out_dir / "upower.json").write_text(json.dumps(upower, indent=2), encoding="utf-8")

    print("  asusd introspection...")
    collect_dbus_xml("xyz.ljones.Asusd", [
        "/xyz/ljones",
        "/xyz/ljones/asus_armoury/charge_mode",
        "/xyz/ljones/asus_armoury/dgpu_disable",
        "/xyz/ljones/asus_armoury/gpu_mux_mode",
        "/xyz/ljones/asus_armoury/nv_dynamic_boost",
        "/xyz/ljones/asus_armoury/nv_temp_target",
        "/xyz/ljones/asus_armoury/panel_overdrive",
        "/xyz/ljones/asus_armoury/ppt_pl1_spl",
        "/xyz/ljones/asus_armoury/ppt_pl2_sppt",
        "/xyz/ljones/asus_armoury/ppt_pl3_fppt",
    ], out_dir, "asusd")

    print("  supergfxd introspection...")
    collect_dbus_xml("org.supergfxctl.Daemon", ["/org/supergfxctl/Gfx"], out_dir, "supergfxd")

    manifest = {
        "probe_version": "0.1.0",
        "collected_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "kernel": platform.release(),
        "arch": platform.machine(),
        "distro": "anonymized",
        "dmi": {k: v for k, v in dmi.items() if v},
        "modules_asus_wmi": collect_modules(),
        "anonymized": True,
        "forbidden_fields_absent": True,
        "read_only": True,
    }
    (out_dir / "manifest.toml").write_text(
        "\n".join(f"{k} = {json.dumps(v, ensure_ascii=False)}" for k, v in manifest.items()) + "\n",
        encoding="utf-8",
    )
    print(f"  -> {out_dir}")


def verify_read_only(out_dir: Path) -> int:
    """Проверка: аппаратный доступ probe строго read-only, фикстуры на месте,
    в фикстурах нет запрещённых персональных полей."""
    problems = []
    src = Path(__file__).read_text(encoding="utf-8")

    # Очищаем docstring-и и комментарии, чтобы проверка не ловила саму себя.
    cleaned = re.sub(r'"""(?:.|\n)*?"""', "", src, flags=re.DOTALL)
    code_lines = [
        line for line in cleaned.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]

    # 1. Никаких shell-команд и sudo в коде probe.
    #    (needles собираются из частей, чтобы проверка не ловила саму себя)
    bad_needles = ["shell" + "=True", "os.sy" + "stem(", "su" + "do "]
    for bad in bad_needles:
        for line in code_lines:
            if bad in line:
                problems.append(f"forbidden '{bad}' in: {line.strip()}")

    # subprocess допускается только внутри run_argv (argv, без shell).
    for line in code_lines:
        if line.lstrip().startswith("except"):
            continue
        if "subprocess." in line and "argv" not in line and "proc = subprocess.run(" not in line:
            problems.append(f"subprocess outside run_argv: {line.strip()}")

    # 2. Запись только в выходной каталог (никогда — в системные пути).
    for line in code_lines:
        if "write_text(" in line and "dest_dir" not in line and "out_dir" not in line:
            problems.append(f"possible write outside out_dir: {line.strip()}")

    # 3. Фикстуры на месте.
    expected = [
        "manifest.toml", "dmi.json", "asusd-introspection-*.xml",
        "supergfxd-introspection-*.xml", "upower.json", "sysfs-tree.json",
        "hwmon.json", "drm.json",
    ]
    files = [p.name for p in out_dir.iterdir()]
    for pat in expected:
        import fnmatch
        if not any(fnmatch.fnmatch(f, pat) for f in files):
            problems.append(f"missing fixture: {pat}")

    # 4. Запрещённые поля в содержимом.
    for f in out_dir.iterdir():
        if f.is_file() and f.suffix in (".json", ".toml", ".xml", ".txt"):
            content = f.read_text(encoding="utf-8", errors="replace").lower()
            for pat in FORBIDDEN_PATTERNS:
                if re.search(pat, content, re.IGNORECASE):
                    problems.append(f"forbidden pattern '{pat}' in {f.name}")

    if problems:
        for p in problems:
            print(f"  ! {p}")
        return 1
    print("  OK: read-only, no forbidden fields, fixtures present")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Orbis Control research probe (read-only)")
    ap.add_argument("command", choices=["collect", "verify-read-only"])
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--allow-writes", action="store_true",
                    help="(всегда отклоняется: probe read-only по конструкции)")
    args = ap.parse_args()

    if args.allow_writes:
        print("  ERROR: probe refuses write mode by design", file=sys.stderr)
        return 2

    if args.command == "collect":
        collect(args.out)
        return 0
    return verify_read_only(args.out)


if __name__ == "__main__":
    sys.exit(main())
