#!/usr/bin/env python3
"""Copy target ELF dependencies into an AppDir without executing target code.

Keep glibc, its dynamic loader, and graphics drivers on the host. GTK, tray and
Slint's dlopen dependencies are included explicitly as well as DT_NEEDED edges.
This also works on x86_64 when the payload is riscv64.
"""

import argparse
from pathlib import Path
import re
import shutil
import subprocess


def bundle(appdir: Path, triplet: str) -> None:
    libdirs = [Path('/usr/lib') / triplet, Path('/lib') / triplet]
    destination = appdir / 'usr/lib'
    destination.mkdir(parents=True, exist_ok=True)
    licenses = appdir / 'usr/share/doc/bundled-libraries'
    licenses.mkdir(parents=True, exist_ok=True)
    # These libraries must agree with the host loader, NSS and GPU drivers.
    system = re.compile(r'^(?:ld-linux.*|lib(?:c|m|pthread|dl|rt|resolv|util|anl|nss_.*)\.so(?:\..*)?)$')
    pending = list((destination / 'bandori-pet-lite').glob('bandori-*'))
    for name in ('libayatana-appindicator3.so.1', 'libxkbcommon-x11.so.0',
                 'libXcursor.so.1', 'libXi.so.6', 'libXrandr.so.2',
                 'libfontconfig.so.1'):
        pending.append(next((directory / name for directory in libdirs
                             if (directory / name).exists()), Path(name)))
    seen = set()
    owners = set()
    while pending:
        source = pending.pop()
        if source.name in seen or system.fullmatch(source.name):
            continue
        if not source.is_file():
            raise SystemExit(f'Missing target library: {source}')
        seen.add(source.name)
        if source.parent != destination / 'bandori-pet-lite':
            shutil.copy2(source, destination / source.name)
            # Ubuntu's merged-/usr layout may still record the /lib alias in dpkg.
            aliases = (source, source.resolve(), Path('/lib') / triplet / source.name,
                       Path('/lib') / triplet / source.resolve().name)
            owner = None
            for alias in aliases:
                result = subprocess.run(['dpkg-query', '-S', str(alias)],
                                        text=True, capture_output=True, check=False)
                if result.returncode == 0:
                    owner = result.stdout.split(': ', 1)[0].split(':', 1)[0]
                    break
            if owner is None:
                raise SystemExit(f'Cannot find the Debian package owning {source}')
            if owner not in owners:
                copyright_file = Path('/usr/share/doc') / owner / 'copyright'
                if not copyright_file.is_file():
                    raise SystemExit(f'Missing copyright for bundled library: {owner}')
                shutil.copy2(copyright_file, licenses / f'{owner}.copyright')
                owners.add(owner)
        dynamic = subprocess.check_output(['readelf', '-d', str(source)], text=True)
        for needed in re.findall(r'\(NEEDED\).*\[([^]]+)\]', dynamic):
            if not system.fullmatch(needed):
                pending.append(next((directory / needed for directory in libdirs
                                     if (directory / needed).is_file()), Path(needed)))
    schemas = Path('/usr/share/glib-2.0/schemas/gschemas.compiled')
    if schemas.is_file():
        target = appdir / 'usr/share/glib-2.0/schemas'
        target.mkdir(parents=True, exist_ok=True)
        shutil.copy2(schemas, target / schemas.name)
    print(f'Bundled {len(seen) - 2} libraries for {triplet}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('appdir', type=Path)
    parser.add_argument('triplet')
    args = parser.parse_args()
    bundle(args.appdir.resolve(), args.triplet)
