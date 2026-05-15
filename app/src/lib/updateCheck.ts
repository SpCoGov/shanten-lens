import {APP_VERSION} from "./version";
import {invoke} from "@tauri-apps/api/core";
import {platform} from "@tauri-apps/plugin-os";

export const UPDATE_REPO_OWNER = "SpCoGov";
export const UPDATE_REPO_NAME = "shanten-lens";
export const UPDATE_RELEASES_URL = `https://github.com/${UPDATE_REPO_OWNER}/${UPDATE_REPO_NAME}/releases`;
const LATEST_RELEASE_API_URL = `https://api.github.com/repos/${UPDATE_REPO_OWNER}/${UPDATE_REPO_NAME}/releases/latest`;

const PREFS_KEY = "sl-update-check:prefs";
const LAST_CHECK_KEY = "sl-update-check:last-check";

export type UpdateAsset = {
    name: string;
    url: string;
    size: number;
};

export type UpdateInfo = {
    version: string;
    name: string;
    body: string;
    publishedAt: string;
    releaseUrl: string;
    downloadUrl: string;
    downloadAssetName: string;
    platform: "windows" | "macos" | "unknown";
    assets: UpdateAsset[];
};

export type UpdateCheckResult =
    | {status: "available"; update: UpdateInfo}
    | {status: "current"; version: string}
    | {status: "disabled"}
    | {status: "ignored"; version: string};

export type UpdateCheckPrefs = {
    autoCheck: boolean;
    useSystemProxy: boolean;
    ignoredVersion: string | null;
};

type GitHubRelease = {
    tag_name?: string;
    name?: string;
    body?: string;
    html_url?: string;
    published_at?: string;
    prerelease?: boolean;
    draft?: boolean;
    assets?: Array<{
        name?: string;
        browser_download_url?: string;
        size?: number;
    }>;
};

export function readUpdatePrefs(): UpdateCheckPrefs {
    try {
        const parsed = JSON.parse(localStorage.getItem(PREFS_KEY) || "{}") as Partial<UpdateCheckPrefs>;
        return {
            autoCheck: parsed.autoCheck !== false,
            useSystemProxy: parsed.useSystemProxy !== false,
            ignoredVersion: typeof parsed.ignoredVersion === "string" && parsed.ignoredVersion ? parsed.ignoredVersion : null,
        };
    } catch {
        return {autoCheck: true, useSystemProxy: true, ignoredVersion: null};
    }
}

export function writeUpdatePrefs(prefs: UpdateCheckPrefs) {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
}

export function setUpdateAutoCheck(autoCheck: boolean) {
    const prefs = readUpdatePrefs();
    writeUpdatePrefs({...prefs, autoCheck});
}

export function setUpdateUseSystemProxy(useSystemProxy: boolean) {
    const prefs = readUpdatePrefs();
    writeUpdatePrefs({...prefs, useSystemProxy});
}

export function ignoreUpdateVersion(version: string) {
    const prefs = readUpdatePrefs();
    writeUpdatePrefs({...prefs, ignoredVersion: version});
}

function normalizeVersion(version: string): string {
    return version.trim().replace(/^[vV]/, "");
}

function compareVersion(a: string, b: string): number {
    const pa = normalizeVersion(a).split(/[.-]/).map((part) => Number.parseInt(part, 10));
    const pb = normalizeVersion(b).split(/[.-]/).map((part) => Number.parseInt(part, 10));
    const len = Math.max(pa.length, pb.length);
    for (let i = 0; i < len; i += 1) {
        const av = Number.isFinite(pa[i]) ? pa[i] : 0;
        const bv = Number.isFinite(pb[i]) ? pb[i] : 0;
        if (av > bv) return 1;
        if (av < bv) return -1;
    }
    return 0;
}

async function currentUpdatePlatform(): Promise<UpdateInfo["platform"]> {
    try {
        const p = await platform();
        if (p === "windows") return "windows";
        if (p === "macos") return "macos";
    } catch {
        // Browser previews fall back to unknown and use the release page.
    }
    return "unknown";
}

function pickDownloadAsset(assets: UpdateAsset[], targetPlatform: UpdateInfo["platform"]): UpdateAsset | null {
    const lowerName = (asset: UpdateAsset) => asset.name.toLowerCase();
    if (targetPlatform === "macos") {
        return assets.find((asset) => lowerName(asset).endsWith(".dmg")) || null;
    }
    if (targetPlatform === "windows") {
        return assets.find((asset) => {
            const name = lowerName(asset);
            return name.endsWith(".zip") && name.includes("portable");
        }) || assets.find((asset) => lowerName(asset).endsWith(".zip")) || null;
    }
    return assets.find((asset) => /\.(msi|exe|zip|dmg)$/i.test(asset.name)) || null;
}

async function parseRelease(release: GitHubRelease): Promise<UpdateInfo | null> {
    const version = normalizeVersion(String(release.tag_name || ""));
    if (!version || release.draft || release.prerelease) return null;
    const assets = (release.assets || [])
        .map((asset) => ({
            name: String(asset.name || ""),
            url: String(asset.browser_download_url || ""),
            size: Number(asset.size || 0),
        }))
        .filter((asset) => asset.name && asset.url);
    const targetPlatform = await currentUpdatePlatform();
    const downloadAsset = pickDownloadAsset(assets, targetPlatform);

    return {
        version,
        name: String(release.name || release.tag_name || `v${version}`),
        body: String(release.body || ""),
        publishedAt: String(release.published_at || ""),
        releaseUrl: String(release.html_url || UPDATE_RELEASES_URL),
        downloadUrl: downloadAsset?.url || String(release.html_url || UPDATE_RELEASES_URL),
        downloadAssetName: downloadAsset?.name || "",
        platform: targetPlatform,
        assets,
    };
}

async function fetchLatestRelease(useSystemProxy: boolean): Promise<GitHubRelease> {
    try {
        const text = await invoke<string>("fetch_latest_release", {useSystemProxy});
        return JSON.parse(text) as GitHubRelease;
    } catch (err) {
        if (!useSystemProxy) throw err;
        const response = await fetch(LATEST_RELEASE_API_URL, {
            headers: {
                Accept: "application/vnd.github+json",
            },
        });
        if (!response.ok) {
            throw new Error(`GitHub Releases API returned ${response.status}`);
        }
        return await response.json() as GitHubRelease;
    }
}

export async function checkForUpdates(options: {manual?: boolean} = {}): Promise<UpdateCheckResult> {
    const prefs = readUpdatePrefs();
    if (!options.manual && !prefs.autoCheck) return {status: "disabled"};

    const update = await parseRelease(await fetchLatestRelease(prefs.useSystemProxy));
    localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
    if (!update) return {status: "current", version: APP_VERSION};

    if (compareVersion(update.version, APP_VERSION) <= 0) {
        return {status: "current", version: update.version};
    }

    if (!options.manual && prefs.ignoredVersion === update.version) {
        return {status: "ignored", version: update.version};
    }

    return {status: "available", update};
}
