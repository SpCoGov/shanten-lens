export type SidebarLayout = {outside: string[]; more: string[]};
export const SIDEBAR_LAYOUT_KEY = "sl-sidebar-layout:v1";
export const separator = (id: string) => id.startsWith("separator:") || id === "spacer";
export function normalizeSidebarLayout(value: unknown, defaults: SidebarLayout): SidebarLayout {
    const saved = value && typeof value === "object" ? value as Partial<SidebarLayout> : defaults;
    const seen = new Set<string>();
    const clean = (items: unknown, zone: keyof SidebarLayout) => (Array.isArray(items) ? items : defaults[zone])
        .filter((id): id is string => typeof id === "string" && id.length > 0 && id.length < 512)
        .filter(id => { if (id === "separator:plugins" || seen.has(id) || (zone === "more" && (id === "more" || id === "spacer"))) return false; seen.add(id); return true; });
    const result = {outside: clean(saved.outside, "outside"), more: clean(saved.more, "more")};
    // Preserve unavailable plugin IDs so re-enabling restores their position.
    for (const zone of ["outside", "more"] as const) {
        for (const id of defaults[zone]) if (!seen.has(id) && !separator(id)) { result[zone].push(id); seen.add(id); }
    }
    if (!seen.has("more")) result.outside.push("more");
    if (!seen.has("spacer")) result.outside.splice(result.outside.indexOf("more") + 1, 0, "spacer");
    return result;
}
