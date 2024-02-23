import { StackContext, Api, Stack, ApiDomainProps } from 'sst/constructs';

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

export async function API({ stack }: StackContext) {
    if (!domain) throw new Error('Missing DOMAIN environment variable');

    const customDomain = getCustomDomain(stack);

    const api = new Api(stack, 'api', {
        defaults: {
            function: {
                runtime: 'rust',
                timeout: '5 minutes',
                environment: {},
            },
        },
        routes: {
            'GET /logs': 'packages/writer/src/log_service_writer.handler',
        },
        customDomain,
    });

    stack.addOutputs({
        ApiEndpoint: api.url,
        host: `https://${customDomain.domainName}`,
    });
}
