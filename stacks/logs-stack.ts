import { SSMClient } from '@aws-sdk/client-ssm';
import { StackContext, Api, Stack, ApiDomainProps } from 'sst/constructs';
import { fetchSstSecret } from '../sst-secrets';

const domainSlug = 'logs';
const domain = process.env.DOMAIN;
const defaultStageName = 'prod';

function getCustomDomain(stack: Stack): ApiDomainProps {
    return {
        domainName:
            stack.stage === defaultStageName
                ? `${domainSlug}.${domain}`
                : `${domainSlug}-${stack.stage}.${domain}`,
        hostedZone: domain,
    };
}

export async function API({ app, stack }: StackContext) {
    if (!domain) throw new Error('Missing DOMAIN environment variable');

    const ssm = new SSMClient({ region: stack.region });

    const customDomain = getCustomDomain(stack);

    const api = new Api(stack, 'api', {
        defaults: {
            function: {
                runtime: 'rust',
                timeout: '5 minutes',
                environment: {
                    LOG_GROUP_NAME: await fetchSstSecret(
                        ssm,
                        app.name,
                        'LOG_GROUP_NAME',
                        app.stage,
                    ),
                    LOG_STREAM_NAME: await fetchSstSecret(
                        ssm,
                        app.name,
                        'LOG_STREAM_NAME',
                        app.stage,
                    ),
                },
                tracing: 'disabled',
            },
        },
        routes: {
            'GET /logs': 'packages/writer/src/free_log_writer.handler',
            'POST /logs': 'packages/writer/src/free_log_writer.handler',
        },
        customDomain,
    });

    stack.addOutputs({
        ApiEndpoint: api.url,
        host: `https://${customDomain.domainName}`,
    });
}
