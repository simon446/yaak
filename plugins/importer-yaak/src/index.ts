import type { Environment, PluginDefinition } from '@yaakapp/api';

export const plugin: PluginDefinition = {
  importer: {
    name: 'Yaak',
    description: 'Yaak official format',
    onImport(_ctx, args) {
      return migrateImport(args.text);
    },
  },
};

export function migrateImport(contents: string) {
  // biome-ignore lint/suspicious/noExplicitAny: none
  let parsed: any;
  try {
    parsed = JSON.parse(contents);
  } catch {
    return undefined;
  }

  if (!isJSObject(parsed)) {
    return undefined;
  }

  const isYaakExport = 'yaakSchema' in parsed;
  if (!isYaakExport) {
    return;
  }

  // Migrate v1 to v2 -- changes requests to httpRequests
  if ('requests' in parsed.resources) {
    parsed.resources.httpRequests = parsed.resources.requests;
    parsed.resources.requests = undefined;
  }

  // Migrate v2 to v3
  for (const workspace of parsed.resources.workspaces ?? []) {
    if ('variables' in workspace) {
      // Create the base environment
      const baseEnvironment: Partial<Environment> = {
        id: `GENERATE_ID::base_env_${workspace.id}`,
        name: 'Global Variables',
        variables: workspace.variables,
        workspaceId: workspace.id,
      };
      parsed.resources.environments = parsed.resources.environments ?? [];
      parsed.resources.environments.push(baseEnvironment);

      // Delete variables key from the workspace
      workspace.variables = undefined;

      // Add environmentId to relevant environments
      for (const environment of parsed.resources.environments) {
        if (environment.workspaceId === workspace.id && environment.id !== baseEnvironment.id) {
          environment.environmentId = baseEnvironment.id;
        }
      }
    }
  }

  // Migrate v3 to v4
  for (const environment of parsed.resources.environments ?? []) {
    if ('environmentId' in environment) {
      environment.base = environment.environmentId == null;
      environment.environmentId = undefined;
    }
  }

  // Migrate v4 to v5
  for (const environment of parsed.resources.environments ?? []) {
    if ('base' in environment && environment.base && environment.parentModel == null) {
      environment.parentModel = 'workspace';
      environment.parentId = null;
      environment.base = undefined;
    } else if ('base' in environment && !environment.base && environment.parentModel == null) {
      environment.parentModel = 'environment';
      environment.parentId = null;
      environment.base = undefined;
    }
  }

  // Migrate v5 to v6 -- path placeholders changed from `:name` to `{name}`. Rewrite parameter
  // names and any matching `:name` occurrences in the URL string.
  for (const list of [parsed.resources.httpRequests ?? [], parsed.resources.websocketRequests ?? []]) {
    for (const req of list) {
      migratePathPlaceholders(req);
    }
  }

  return { resources: parsed.resources };
}

// biome-ignore lint/suspicious/noExplicitAny: legacy import format is untyped
function migratePathPlaceholders(req: any) {
  if (!Array.isArray(req?.urlParameters)) return;
  const renames: string[] = [];
  for (const p of req.urlParameters) {
    if (typeof p?.name === 'string' && p.name.startsWith(':') && p.name.length > 1) {
      const newName = `{${p.name.slice(1)}}`;
      renames.push(p.name);
      p.name = newName;
    }
  }
  if (renames.length === 0) return;
  if (typeof req.url !== 'string') return;
  for (const oldName of renames) {
    const bareName = oldName.slice(1);
    const escaped = bareName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    req.url = req.url.replace(new RegExp(`:${escaped}(?=[/?#]|$)`, 'g'), `{${bareName}}`);
  }
}

function isJSObject(obj: unknown) {
  return Object.prototype.toString.call(obj) === '[object Object]';
}
