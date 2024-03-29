import { SSTConfig } from 'sst';
import { API } from './stacks/logs-stack';

export default {
    config(_input) {
        return {
            name: 'log-service',
            region: 'us-east-1',
        };
    },
    async stacks(app) {
        await app.stack(API);
    },
} satisfies SSTConfig;
