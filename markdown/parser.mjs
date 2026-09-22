// Syntax only: no Terra paths, IDs, mutations, or HTML rendering.
import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkGfm from 'remark-gfm';
export { toString as headingText } from 'mdast-util-to-string';

const processor = unified().use(remarkParse).use(remarkGfm);
// For derived analysis of already-stored text; ingestion limits belong to callers.
export const parseTree = source => processor.parse(source);

/** CommonMark + GFM AST with original UTF-16 source offsets.
 * Keep source slices instead of reserializing the AST: indentation, markers,
 * escapes and CRLF inside each block survive import. Positions belong only to
 * this input string, never to a subsequently edited Terra document.
 */
export function parseMarkdown(source) {
  if (typeof source !== 'string' || source.length > 250000 || !source.trim()) {
    throw Error('Supply 1–250000 characters of Markdown.');
  }
  const tree = parseTree(source);
  function slice(node) {
    let start = node.position.start.offset;
    const end = node.position.end.offset;
    const lineStart = source.lastIndexOf('\n', start - 1) + 1;
    // AST nodes may start after indentation. Preserve it without swallowing
    // structural text (such as an enclosing quote/list marker).
    if (/^[ \t]*$/.test(source.slice(lineStart, start))) start = lineStart;
    return source.slice(start, end);
  }
  return { source, tree, slice };
}
