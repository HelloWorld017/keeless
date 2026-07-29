const needsQuotes = /[\s"\\]/;

export const searchFilterToken = (prefix: 'in' | 'tag', value: string) => {
  const escaped = value.replaceAll('\\', '\\\\').replaceAll('"', '\\"');
  return `${prefix}:${needsQuotes.test(value) ? `"${escaped}"` : escaped}`;
};
