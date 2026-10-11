import {readFileSync, readdirSync, writeFileSync} from "node:fs";
import {basename, resolve, join} from "node:path";
import {pathToFileURL} from "node:url";

export function createUpdateManifest(directory, version, repository) {
    if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?$/.test(version)) {
        throw new Error("Invalid release version");
    }
    if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error("Invalid repository");
    const installers = readdirSync(directory, {recursive: true}).filter((name) => name.endsWith("_x64-setup.exe"));
    if (installers.length !== 1 || !basename(installers[0]).includes(`_${version}_`)) {
        throw new Error("Expected exactly one Windows x64 installer matching the release version");
    }
    const installer = installers[0];
    const signature = readFileSync(join(directory, `${installer}.sig`), "utf8").trim();
    if (!signature) throw new Error("Missing update signature");
    return {
        version,
        platforms: {
            "windows-x86_64": {
                signature,
                url: `https://github.com/${repository}/releases/download/v${version}/${encodeURIComponent(basename(installer).replaceAll(" ", "."))}`,
            },
        },
    };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
    const directory = resolve(process.argv[2] || "artifacts");
    const version = readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8")
        .match(/^version\s*=\s*"([^"]+)"/m)?.[1];
    const manifest = createUpdateManifest(directory, version, process.env.GITHUB_REPOSITORY);
    writeFileSync(join(directory, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}
