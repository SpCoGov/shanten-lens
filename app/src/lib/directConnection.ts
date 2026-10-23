import {invoke} from "@tauri-apps/api/core";

export type DirectRegion = "cn" | "jp" | "en" | "kr";
export type DirectAccount = {
    id: string;
    label: string;
    region: DirectRegion;
    login_method: "password" | "oauth" | "email";
    username: string;
    uid: string;
    oauth_type: number;
    client_version_string: string;
    route: string | null;
    has_password: boolean;
    has_code: boolean;
    has_session: boolean;
};
export type DirectAccountInput = Omit<DirectAccount, "id" | "has_password" | "has_code" | "has_session"> & {
    id?: string;
    password?: string;
    code?: string;
    verification_code?: string;
};
export type DirectStatus = {
    state: "disconnected" | "connecting" | "connected" | "error";
    account_id: string | null;
    route: string | null;
    last_heartbeat_ms: number | null;
    error: string | null;
    capture_account_id: string | null;
};
export type DirectRoute = {id: string; endpoint: string; latency_ms: number | null};

export function accountInput(account: DirectAccountInput): DirectAccountInput {
    const {password, code, verification_code, ...fields} = account;
    return {...fields, label: fields.label.trim(), username: fields.username.trim(), uid: fields.uid.trim(),
        ...(fields.login_method === "password" && password ? {password} : {}),
        ...(fields.login_method === "oauth" && code?.trim() ? {code: code.trim()} : {}),
        ...(fields.login_method === "email" && verification_code?.trim() ? {verification_code: verification_code.trim()} : {})};
}

export const getDirectAccounts = () => invoke<DirectAccount[]>("backend_direct_accounts");
export const saveDirectAccount = (account: DirectAccountInput) => invoke<DirectAccount[]>("backend_save_direct_account", {account: accountInput(account)});
export const deleteDirectAccount = (id: string) => invoke<DirectAccount[]>("backend_delete_direct_account", {id});
export const getDirectRoutes = (region: DirectRegion) => invoke<DirectRoute[]>("backend_direct_routes", {region});
export const getDirectStatus = () => invoke<DirectStatus>("backend_direct_status");
export const connectDirect = (accountId: string) => invoke<DirectStatus>("backend_connect_direct", {accountId});
export const disconnectDirect = () => invoke<DirectStatus>("backend_disconnect_direct");
export const captureDirectAccount = (accountId: string | null) => invoke<DirectStatus>("backend_capture_direct_account", {accountId});
export const sendDirectEmailCode = (region: DirectRegion, email: string) => invoke<void>("backend_send_direct_email_code", {region, email: email.trim()});
