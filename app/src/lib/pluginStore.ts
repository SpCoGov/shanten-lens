import {create} from "zustand";

export type PluginPage = {
    id: string;
    pluginId: string;
    title: string | (() => string);
    icon: string;
    mount(container: HTMLElement): void | (() => void) | Promise<void | (() => void)>;
};

type PluginUiState = {
    pages: PluginPage[];
    errors: Record<string, string>;
};

export const usePluginStore = create<PluginUiState>(() => ({pages: [], errors: {}}));

export function registerPluginPage(page: PluginPage) {
    usePluginStore.setState((state) => ({
        pages: [...state.pages.filter((item) => item.id !== page.id), page],
    }));
    return () => usePluginStore.setState((state) => ({
        pages: state.pages.filter((item) => item !== page),
    }));
}

export function clearPluginUi(pluginId: string) {
    usePluginStore.setState((state) => ({
        pages: state.pages.filter((page) => page.pluginId !== pluginId),
    }));
}

export function setPluginFrontendError(pluginId: string, error?: string) {
    usePluginStore.setState((state) => {
        const errors = {...state.errors};
        if (error) errors[pluginId] = error;
        else delete errors[pluginId];
        return {errors};
    });
}
