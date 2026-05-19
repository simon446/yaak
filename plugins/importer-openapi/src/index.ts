import { convertPostman } from '@yaak/importer-postman/src';
import type { Context, HttpRequest, PluginDefinition } from '@yaakapp/api';
import type { ImportPluginResponse } from '@yaakapp/api/lib/plugins/ImporterPlugin';
import { convert } from 'openapi-to-postmanv2';
import { parse as parseYaml } from 'yaml';

export const plugin: PluginDefinition = {
  importer: {
    name: 'OpenAPI',
    description: 'Import OpenAPI collections',
    onImport(_ctx: Context, args: { text: string }) {
      return convertOpenApi(args.text);
    },
  },
};

export async function convertOpenApi(contents: string): Promise<ImportPluginResponse | undefined> {
  // biome-ignore lint/suspicious/noExplicitAny: none
  let postmanCollection: any;
  try {
    postmanCollection = await new Promise((resolve, reject) => {
      // biome-ignore lint/suspicious/noExplicitAny: none
      convert({ type: 'string', data: contents }, {}, (err, result: any) => {
        if (err != null) reject(err);

        if (Array.isArray(result.output) && result.output.length > 0) {
          resolve(result.output[0].data);
        }
      });
    });
  } catch {
    // Probably not an OpenAPI file, so skip it
    return undefined;
  }

  const converted = convertPostman(JSON.stringify(postmanCollection));
  if (converted == null) return converted;

  // Rescue path parameters that `openapi-to-postman` failed to extract. This happens for
  // OpenAPI paths that contain literal `:` (e.g. `/tasks/{id}:increment-importance` — a
  // Google AIP-136 custom method), where the library emits the placeholder as a Postman
  // environment variable (`{{id}}`) instead of a path variable. We walk the original
  // OpenAPI spec for `{name}` placeholders in path keys and rewrite the corresponding
  // `${[name]}` references in the converted URLs back into path placeholders.
  const pathParamNames = collectOpenApiPathParamNames(contents);
  if (pathParamNames.size > 0) {
    for (const req of converted.resources.httpRequests ?? []) {
      rescuePathParameters(req, pathParamNames);
    }
  }

  return converted;
}

function collectOpenApiPathParamNames(contents: string): Set<string> {
  const names = new Set<string>();
  let spec: unknown;
  try {
    spec = parseYaml(contents);
  } catch {
    return names;
  }
  if (spec == null || typeof spec !== 'object') return names;
  const paths = (spec as { paths?: unknown }).paths;
  if (paths == null || typeof paths !== 'object') return names;
  for (const key of Object.keys(paths)) {
    for (const m of key.matchAll(/\{([^/?#}]+)\}/g)) {
      if (m[1] != null) names.add(m[1]);
    }
  }
  return names;
}

function rescuePathParameters(
  // biome-ignore lint/suspicious/noExplicitAny: partial type from importer
  req: any,
  pathParamNames: Set<string>,
) {
  if (typeof req.url !== 'string') return;
  const existingNames = new Set(
    (req.urlParameters ?? []).map((p: HttpRequest['urlParameters'][number]) => p.name),
  );

  let url: string = req.url;
  const re = /\$\{\[([^\]]+)\]\}/g;
  const rescued = new Set<string>();
  url = url.replace(re, (match, name: string) => {
    if (!pathParamNames.has(name)) return match;
    rescued.add(name);
    return `{${name}}`;
  });
  if (rescued.size === 0) return;

  req.url = url;
  req.urlParameters = [...(req.urlParameters ?? [])];
  for (const name of rescued) {
    const braceName = `{${name}}`;
    if (existingNames.has(braceName)) continue;
    req.urlParameters.push({ name: braceName, value: '', enabled: true });
  }

  // The request `name` is often the literal OpenAPI path — keep it in sync.
  if (typeof req.name === 'string') {
    req.name = req.name.replace(re, (match: string, name: string) =>
      rescued.has(name) ? `{${name}}` : match,
    );
  }
}
