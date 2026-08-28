/// <reference types="vite/client" />

declare const __APP_VERSION__: string;

declare module "virtual:liqi-methods" {
    const methods: string[];
    export default methods;
}
