"""Build packages for the host OS from the release binaries."""
import os
import platform
import shutil
import subprocess
import tempfile
import uuid
from pathlib import Path

root = Path(__file__).resolve().parents[1]
dist = root / "dist"
dist.mkdir(exist_ok=True)
version = "1.2.6"

def run(*args):
    subprocess.run(args, check=True)

def package(binary, stage):
    system = platform.system()
    source = root / "target/release" / (binary + (".exe" if system == "Windows" else ""))
    client = binary == "emerald"
    title = "Emerald" if client else "Emerald Server"
    if system == "Linux":
        dest = stage / "usr/bin"
        dest.mkdir(parents=True)
        shutil.copy2(source, dest)
        if client:
            apps = stage / "usr/share/applications"
            apps.mkdir(parents=True)
            shutil.copy2(root / "packaging/dev.emerald.Vault.desktop", apps)
            icons = stage / "usr/share/icons/hicolor/256x256/apps"
            icons.mkdir(parents=True)
            shutil.copy2(root / "assets/emerald-256.png", icons / "emerald.png")
        meta = stage / "DEBIAN"
        meta.mkdir()
        arch = subprocess.check_output(["dpkg", "--print-architecture"], text=True).strip()
        (meta / "control").write_text(f"Package: {binary}\nVersion: {version}\nArchitecture: {arch}\nMaintainer: Galitube Productions\nDescription: {title} Markdown vault\n")
        stage.chmod(0o755)
        for item in stage.rglob("*"):
            item.chmod(0o755 if item.is_dir() or item == dest / binary else 0o644)
        run("dpkg-deb", "--root-owner-group", "--build", stage, dist / f"{binary}-{arch}.deb")
    elif system == "Darwin":
        import plistlib
        if client:
            app = stage / "Emerald.app/Contents"
            (app / "Resources").mkdir(parents=True)
            dest = app / "MacOS"
            dest.mkdir(parents=True)
            shutil.copy2(source, dest)
            iconset = stage / "Emerald.iconset"
            iconset.mkdir()
            for size in (16, 32, 128, 256, 512):
                shutil.copy2(root / f"assets/emerald-{size}.png", iconset / f"icon_{size}x{size}.png")
                if size < 512:
                    shutil.copy2(root / f"assets/emerald-{size*2}.png", iconset / f"icon_{size}x{size}@2x.png")
            run("iconutil", "-c", "icns", iconset, "-o", app / "Resources/AppIcon.icns")
            shutil.rmtree(iconset)
            (app / "Info.plist").write_bytes(plistlib.dumps(dict(CFBundleExecutable=binary, CFBundleIdentifier="dev.emerald.vault", CFBundlePackageType="APPL", CFBundleName=title, CFBundleVersion=version, CFBundleIconFile="AppIcon.icns")))
            os.symlink("/Applications", stage / "Applications")
        else:
            dest = stage / "payload/usr/local/bin"
            dest.mkdir(parents=True)
            shutil.copy2(source, dest)
            run("pkgbuild", "--root", stage / "payload", "--identifier", "dev.emerald.server", "--version", version, stage / "Emerald Server.pkg")
            shutil.rmtree(stage / "payload")
        run("hdiutil", "create", "-ov", "-volname", title, "-srcfolder", stage, dist / f"{binary}-{platform.machine().replace('aarch64','arm64')}.dmg")
    elif system == "Windows":
        upgrade = uuid.uuid5(uuid.NAMESPACE_DNS, binary+".dev.emerald")
        shortcut = ('<Shortcut Id="Launch" Directory="ProgramMenuFolder" Name="Emerald" Advertise="yes" Icon="EmeraldIcon"/>' if client else '')
        xml = f'''<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs"><Package Name="{title}" Manufacturer="Galitube Productions" Version="{version}" UpgradeCode="{upgrade}" Scope="perMachine"><MajorUpgrade DowngradeErrorMessage="A newer version is installed."/><MediaTemplate EmbedCab="yes"/><StandardDirectory Id="ProgramMenuFolder"/><Icon Id="EmeraldIcon" SourceFile="{root / "assets/emerald.ico"}"/><Property Id="ARPPRODUCTICON" Value="EmeraldIcon"/><StandardDirectory Id="ProgramFiles64Folder"><Directory Id="INSTALL" Name="{title}"><Component Id="App" Guid="*"><File Source="{source}" KeyPath="yes">{shortcut}</File></Component></Directory></StandardDirectory><Feature Id="Main"><ComponentRef Id="App"/></Feature></Package></Wix>'''
        manifest = stage / "installer.wxs"
        manifest.write_text(xml)
        run("wix", "build", "-arch", "x64", manifest, "-o", dist / f"{binary}.msi")
    else:
        raise RuntimeError(system)

for binary in ("emerald", "emerald-server"):
    with tempfile.TemporaryDirectory() as temp:
        package(binary, Path(temp))
