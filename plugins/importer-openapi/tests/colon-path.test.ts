import { describe, expect, test } from 'vitest';
import { convertOpenApi } from '../src';

describe('importer-openapi: colon-in-path', () => {
  test('rescues a path parameter when the path also has a literal `:` (AIP-136 custom method)', async () => {
    const spec = {
      openapi: '3.0.0',
      info: { title: 'T', version: '1.0.0' },
      paths: {
        '/tasks/{id}:increment-importance': {
          post: {
            parameters: [{ name: 'id', in: 'path', required: true, schema: { type: 'string' } }],
            responses: { '200': { description: 'ok' } },
          },
        },
      },
    };
    const imported = await convertOpenApi(JSON.stringify(spec));
    const req = imported?.resources.httpRequests?.[0];
    expect(req?.url).toBe('${[baseUrl]}/tasks/{id}:increment-importance');
    expect(req?.name).toBe('/tasks/{id}:increment-importance');
    expect(req?.urlParameters).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ name: '{id}', enabled: true }),
      ]),
    );
  });

  test('normal path parameters still flow through correctly', async () => {
    const spec = {
      openapi: '3.0.0',
      info: { title: 'T', version: '1.0.0' },
      paths: {
        '/tasks/{id}': {
          get: {
            parameters: [{ name: 'id', in: 'path', required: true, schema: { type: 'string' } }],
            responses: { '200': { description: 'ok' } },
          },
        },
      },
    };
    const imported = await convertOpenApi(JSON.stringify(spec));
    const req = imported?.resources.httpRequests?.[0];
    expect(req?.url).toBe('${[baseUrl]}/tasks/{id}');
    // openapi-to-postman names the item `/tasks/:id` — make sure we rewrite the
    // request name to match the URL's `{id}` form.
    expect(req?.name).toBe('/tasks/{id}');
    expect(req?.urlParameters).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ name: '{id}' }),
      ]),
    );
  });
});
