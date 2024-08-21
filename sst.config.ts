/// <reference path="./.sst/platform/config.d.ts" />

export default $config({
    app(input) {
        return {
            name: 'freelog',
            removal: input?.stage === 'prod' ? 'retain' : 'remove',
            home: 'aws',
            providers: { aws: { region: 'us-east-1' } },
        };
    },
    async run() {
        const api = await import('./infra/api');

        return { ...api.default };
    },
});
