import { describe, expect, test } from 'vitest';
import { extractPathPlaceholders } from './pathPlaceholders';

describe('extractPathPlaceholders', () => {
  test('extracts a single placeholder', () => {
    expect(extractPathPlaceholders('/users/{id}')).toEqual(['{id}']);
  });

  test('extracts multiple placeholders', () => {
    expect(extractPathPlaceholders('/users/{id}/posts/{postId}')).toEqual(['{id}', '{postId}']);
  });

  test('does not match while the closing brace is still missing', () => {
    // The motivating UX fix: typing `/users/{ab` should not yet add anything to
    // the parameter list.
    expect(extractPathPlaceholders('/users/{ab')).toEqual([]);
  });

  test('does not match empty braces', () => {
    expect(extractPathPlaceholders('/users/{}')).toEqual([]);
  });

  test('does not span path separators', () => {
    expect(extractPathPlaceholders('/users/{a/b}')).toEqual([]);
  });

  test('handles a placeholder followed by a literal `:` (AIP-136 custom method)', () => {
    expect(extractPathPlaceholders('/tasks/{id}:increment-importance')).toEqual(['{id}']);
  });

  test('legacy `:name` syntax is ignored', () => {
    expect(extractPathPlaceholders('/users/:id')).toEqual([]);
  });

  test('returns an empty array for a URL with no placeholders', () => {
    expect(extractPathPlaceholders('https://example.com/foo/bar?q=1#hash')).toEqual([]);
  });

  test('does not capture the inner braces of an env-var template tag', () => {
    // `${[ host ]}` contains a `{...}` pair if you look at just the braces, but
    // it's an env-var reference, not a path placeholder.
    expect(extractPathPlaceholders('https://${[ host ]}/users/{id}')).toEqual(['{id}']);
  });

  test('handles env-var tags with no surrounding whitespace', () => {
    expect(extractPathPlaceholders('https://${[host]}/users/{id}')).toEqual(['{id}']);
  });

  test('handles multiple env-var tags around a placeholder', () => {
    expect(extractPathPlaceholders('${[ scheme ]}://${[ host ]}/v1/users/{userId}/items/{itemId}'))
      .toEqual(['{userId}', '{itemId}']);
  });

  test('does not match a placeholder that sits inside an env-var tag', () => {
    // Pathological but worth nailing down: braces inside an env-var tag are
    // structurally inert, even if they look like a placeholder.
    expect(extractPathPlaceholders('https://example.com/${[ {nested} ]}/x')).toEqual([]);
  });
});
