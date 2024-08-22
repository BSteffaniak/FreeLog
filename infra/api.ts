import { ApiGatewayV2DomainArgs } from '@sst/platform/src/components/aws/helpers/apigatewayv2-domain';

const domainSlug = 'logs';
const domain = process.env.DOMAIN;
const hostedZone = process.env.HOSTED_ZONE;
const defaultStageName = 'prod';

function getCustomDomain(): ApiGatewayV2DomainArgs {
    return {
        name:
            $app.stage === defaultStageName
                ? `${domainSlug}.${domain}`
                : `${domainSlug}-${$app.stage}.${domain}`,
        dns: sst.aws.dns({
            zone: hostedZone,
        }),
    };
}

if (!domain) throw new Error('Missing DOMAIN environment variable');
if (!hostedZone) throw new Error('Missing HOSTED_ZONE environment variable');

const customDomain = getCustomDomain();

const logGroupName = new sst.Secret('LogGroupName', 'freelog_logs');
const logStreamName = new sst.Secret('LogStreamName', 'stream_1');

const api = new sst.aws.ApiGatewayV2('api', {
    transform: {
        route: {
            handler: {
                // runtime: 'rust',
                timeout: '5 minutes',
                environment: {
                    LogGroupName: $interpolate`${logGroupName}`,
                    LogStreamName: $interpolate`${logStreamName}`,
                },
            },
        },
    },
    domain: customDomain,
});

api.route('GET /logs', 'packages/writer/src/free_log_writer.handler');
api.route('POST /logs', 'packages/writer/src/free_log_writer.handler');

export const outputs = {
    ApiEndpoint: api.url,
    host: `https://${customDomain.name}`,
};
