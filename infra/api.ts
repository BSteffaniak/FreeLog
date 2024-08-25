import { ApiGatewayV2DomainArgs } from '@sst/platform/src/components/aws/helpers/apigatewayv2-domain';
import { exec } from 'node:child_process';

const domainSlug = 'logs';
const domain = process.env.DOMAIN;
const defaultStageName = 'prod';

async function getHostedZoneId(domain: string): Promise<string> {
    return new Promise((resolve, reject) => {
        exec(
            `aws route53 list-hosted-zones-by-name --query "HostedZones[?Name=='${domain}.'].Id"  --output text | sed s#/hostedzone/##`,
            (error, stdout, stderr) => {
                if (error) {
                    console.error(stderr);
                    return reject(error);
                }
                resolve(stdout.trim());
            },
        );
    });
}

function getCustomDomain(hostedZoneId: string): ApiGatewayV2DomainArgs {
    return {
        name:
            $app.stage === defaultStageName
                ? `${domainSlug}.${domain}`
                : `${domainSlug}-${$app.stage}.${domain}`,
        dns: sst.aws.dns({
            zone: hostedZoneId,
        }),
    };
}

if (!domain) throw new Error('Missing DOMAIN environment variable');

const hostedZoneId = await getHostedZoneId(domain);
const customDomain = getCustomDomain(hostedZoneId);

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
