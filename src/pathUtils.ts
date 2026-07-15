export function topName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function duplicateTopNames(paths: string[]) {
  const counts = new Map<string, number>();
  for (const path of paths) {
    const normalized = topName(path).toLocaleLowerCase();
    counts.set(normalized, (counts.get(normalized) ?? 0) + 1);
  }
  return new Set([...counts].filter(([, count]) => count > 1).map(([name]) => name));
}
