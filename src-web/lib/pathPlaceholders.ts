/**
 * Extract brace-delimited path placeholders (`{name}`) from a URL string.
 *
 * The closing `}` is required, so partially-typed input like `/users/{ab` matches
 * nothing — only fully-formed `{name}` segments are returned. A placeholder cannot
 * span `/`, `?`, `#`, or `}` characters, so it's always confined to a single path
 * segment.
 *
 * Env-var / template-function tags (`${[ name ]}`) are stripped before scanning so
 * their inner `{...}` shape isn't mis-parsed as a path placeholder. The lazy `.*?`
 * stops at the first `]}` (the same closing token Twig uses).
 */
export function extractPathPlaceholders(url: string): string[] {
  const withoutTags = url.replace(/\$\{\[.*?\]\}/g, '');
  return Array.from(withoutTags.matchAll(/(\{[^/?#}]+\})/g)).map((m) => m[1] ?? '');
}
