/**
 * 命名変換 helpers — KDL 側 kebab-case / snake_case を TS 側 PascalCase / camelCase に変換。
 */

/** kebab-case / snake_case → PascalCase (例: "memories-atlas" → "MemoriesAtlas") */
export function pascalCase(s: string): string {
  return s
    .split(/[-_]/)
    .filter(Boolean)
    .map(part => part.charAt(0).toUpperCase() + part.slice(1))
    .join('')
}

/** snake_case → camelCase (例: "display_name" → "displayName")。 field 名は KDL 側の snake_case を維持したい場合もあるので caller 選択可能 */
export function camelCase(s: string): string {
  const [head, ...rest] = s.split(/[-_]/).filter(Boolean)
  if (!head) return ''
  return (
    head.charAt(0).toLowerCase() +
    head.slice(1) +
    rest.map(p => p.charAt(0).toUpperCase() + p.slice(1)).join('')
  )
}
