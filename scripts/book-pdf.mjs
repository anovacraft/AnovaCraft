// Print an mdBook's print.html to a PDF whose table of contents, chapter
// links and cross-references are live links, with a bookmark outline.
// Run by scripts/build-manual-pdfs.sh, which builds both books first.
//
//   node scripts/book-pdf.mjs BOOK_HTML_DIR OUT.pdf "Title" REPO_SRC_PATH
//
// BOOK_HTML_DIR is the folder `mdbook build -d` wrote; REPO_SRC_PATH is the
// book's source folder in the repository (docs/manual/src), used to turn
// links into the other book into links to the files on GitHub.
//
// Pictures are re-encoded as JPEG (quality MANUAL_JPEG_QUALITY, default 0.65)
// at most MANUAL_IMAGE_WIDTH pixels wide (default 1100), which keeps the PDFs
// small enough to build into the app; the books keep their PNG screenshots.
import { execSync } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

// Playwright from this folder's node_modules or the global npm folder.
const require = createRequire(import.meta.url);
const globalRoot = (() => {
  try { return execSync('npm root -g', { encoding: 'utf8' }).trim(); } catch { return ''; }
})();
const { chromium } = require(require.resolve('playwright', { paths: [process.cwd(), globalRoot].filter(Boolean) }));

const [dir, out, title, srcPath] = process.argv.slice(2);
const imageWidth = Number(process.env.MANUAL_IMAGE_WIDTH || 1100);
const jpegQuality = Number(process.env.MANUAL_JPEG_QUALITY || 0.65);
// File pictures may be read back from a canvas only with this flag.
const browser = await chromium.launch({ args: ['--allow-file-access-from-files'] });
const page = await browser.newPage();
// print.html calls window.print() when it loads.
await page.addInitScript(() => { window.print = () => {}; });
await page.goto(pathToFileURL(path.join(path.resolve(dir), 'print.html')).href, { waitUntil: 'load' });
// MathJax, if the book uses it.
await page.evaluate(async () => {
  if (window.MathJax && window.MathJax.Hub) {
    await new Promise((done) => window.MathJax.Hub.Queue(done));
  }
});
const report = await page.evaluate(({ title, srcPath }) => {
  const main = document.querySelector('main');
  const root = new URL('./', location.href);
  // Chapters, in the sidebar's order (part titles have no link).
  const entries = [];
  const walk = (ol, depth) => {
    for (const li of ol.children) {
      if (li.classList.contains('part-title')) {
        entries.push({ part: li.textContent.trim(), depth });
        continue;
      }
      const a = li.querySelector(':scope > a');
      if (a && a.getAttribute('href')) {
        const label = a.cloneNode(true);
        label.querySelectorAll('strong').forEach((s) => s.remove());
        entries.push({ href: new URL(a.getAttribute('href'), root).href, label: label.textContent.trim(), number: (a.querySelector('strong') || {}).textContent || '', depth });
      }
      const sub = li.querySelector(':scope > ol');
      if (sub) walk(sub, depth + 1);
    }
  };
  walk(document.querySelector('#sidebar ol.chapter'), 0);
  const chapters = entries.filter((e) => e.href);

  // Split the page at the page breaks mdBook puts between chapters.
  const starts = [main.firstElementChild];
  for (const el of main.querySelectorAll(':scope > div[style*="break-before"]')) starts.push(el);
  if (starts.length !== chapters.length) {
    return { error: `chapters ${chapters.length} but page sections ${starts.length}` };
  }
  const byPath = new Map();
  starts.forEach((start, i) => {
    const id = `chapter-${i + 1}`;
    const anchor = document.createElement('a');
    anchor.id = id;
    start.parentNode.insertBefore(anchor, start.tagName === 'DIV' ? start.nextSibling : start);
    chapters[i].id = id;
    // The chapter's elements, to find a fragment within it.
    const els = [];
    let el = start;
    do {
      els.push(el);
      el = el.nextElementSibling;
    } while (el && !(el.tagName === 'DIV' && (el.getAttribute('style') || '').includes('break-before')));
    chapters[i].els = els;
    byPath.set(chapters[i].href.split('#')[0], chapters[i]);
  });
  const findIn = (chapter, frag) => {
    for (const el of chapter.els) {
      if (el.id === frag) return el.id;
      const hit = el.querySelector(`[id="${CSS.escape(frag)}"]`);
      if (hit) return hit.id;
    }
    // mdBook can add a suffix to a repeated id.
    for (const el of chapter.els) {
      const hit = el.id && el.id.startsWith(frag) ? el : el.querySelector(`[id^="${CSS.escape(frag)}"]`);
      if (hit) return hit.id;
    }
    return null;
  };
  let internal = 0, external = 0, broken = [];
  const repo = 'https://github.com/anovacraft/AnovaCraft/blob/main/';
  for (const a of main.querySelectorAll('a[href]')) {
    const raw = a.getAttribute('href');
    if (raw.startsWith('#') || /^[a-z]+:/i.test(raw) && !raw.startsWith('file:')) continue;
    const url = new URL(raw, root);
    const base = url.href.split('#')[0];
    const frag = url.hash ? decodeURIComponent(url.hash.slice(1)) : '';
    const chapter = byPath.get(base);
    if (chapter) {
      const id = frag ? findIn(chapter, frag) : chapter.id;
      if (!id) broken.push(raw);
      a.setAttribute('href', '#' + (id || chapter.id));
      internal++;
    } else if (base.startsWith(root.href) || url.protocol === 'file:') {
      // Outside this book: the source file on GitHub.
      const rel = path_rel(root.href, base);
      const md = (srcPath + '/' + rel).split('/').reduce((acc, part) => {
        if (part === '..') acc.pop(); else if (part && part !== '.') acc.push(part);
        return acc;
      }, []).join('/').replace(/\.html$/, '.md');
      a.setAttribute('href', repo + md + (frag ? '#' + frag : ''));
      external++;
    }
  }
  function path_rel(from, to) {
    // `to` relative to folder URL `from` (may climb out with ..).
    const f = from.split('/').slice(0, -1), t = to.split('/');
    let i = 0;
    while (i < f.length && f[i] === t[i]) i++;
    return [...Array(f.length - i).fill('..'), ...t.slice(i)].join('/');
  }

  // Title page and contents.
  const front = document.createElement('section');
  front.className = 'pdf-front';
  const h = document.createElement('div');
  h.className = 'pdf-title';
  h.textContent = title;
  front.appendChild(h);
  const sub = document.createElement('div');
  sub.className = 'pdf-subtitle';
  sub.textContent = 'Generated ' + new Date().toISOString().slice(0, 10) + ' from the AnovaCraft repository';
  front.appendChild(sub);
  const toc = document.createElement('nav');
  toc.className = 'pdf-toc';
  const th = document.createElement('div');
  th.className = 'pdf-toc-heading';
  th.textContent = 'Contents';
  toc.appendChild(th);
  for (const e of entries) {
    const row = document.createElement('div');
    if (e.part) {
      row.className = 'pdf-toc-part';
      row.textContent = e.part;
    } else {
      row.className = 'pdf-toc-entry';
      row.style.paddingLeft = (e.depth * 1.4) + 'em';
      const link = document.createElement('a');
      link.href = '#' + e.id;
      link.textContent = (e.number ? e.number + ' ' : '') + e.label;
      row.appendChild(link);
    }
    toc.appendChild(row);
  }
  front.appendChild(toc);
  main.insertBefore(front, main.firstChild);
  const style = document.createElement('style');
  style.textContent = `
    .pdf-front { break-after: page; }
    .pdf-title { font-size: 2.6em; font-weight: 700; margin: 2.5em 0 0.3em; }
    .pdf-subtitle { color: #666; margin-bottom: 3em; }
    .pdf-toc-heading { font-size: 1.6em; font-weight: 700; margin-bottom: 0.6em; break-before: page; }
    .pdf-toc-part { font-weight: 700; margin: 0.9em 0 0.2em; }
    .pdf-toc-entry { margin: 0.12em 0; }
    .pdf-toc a { text-decoration: none; color: #1f4e8c; }
    main a { color: #1f4e8c; }
    img { max-width: 100%; height: auto; break-inside: avoid; }
    pre, blockquote, tr { break-inside: avoid; }
    h1, h2, h3 { break-after: avoid; }
    @page { margin: 16mm 14mm 18mm; }
  `;
  document.head.appendChild(style);
  return { chapters: chapters.length, internal, external, broken };
}, { title, srcPath });
console.log(JSON.stringify(report));
if (report.error) process.exit(1);
// Wait for every picture.
await page.evaluate(async () => {
  await Promise.all([...document.images].map((img) => img.complete ? null : new Promise((r) => { img.onload = img.onerror = r; })));
});
// Re-encode them as JPEG on a white ground (the books' pages are white).
const shrunk = await page.evaluate(async ({ imageWidth, jpegQuality }) => {
  let n = 0;
  for (const img of document.images) {
    if (!img.naturalWidth || /\.svg($|\?)|^data:image\/svg/.test(img.src)) continue;
    const scale = Math.min(1, imageWidth / img.naturalWidth);
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(img.naturalWidth * scale);
    canvas.height = Math.round(img.naturalHeight * scale);
    const g = canvas.getContext('2d');
    g.fillStyle = '#ffffff';
    g.fillRect(0, 0, canvas.width, canvas.height);
    g.imageSmoothingQuality = 'high';
    g.drawImage(img, 0, 0, canvas.width, canvas.height);
    const done = new Promise((r) => { img.onload = img.onerror = r; });
    img.src = canvas.toDataURL('image/jpeg', jpegQuality);
    await done;
    n++;
  }
  return n;
}, { imageWidth, jpegQuality });
console.log(JSON.stringify({ jpegPictures: shrunk }));
await page.pdf({
  path: out,
  format: 'Letter',
  printBackground: true,
  outline: true,
  tagged: true,
  displayHeaderFooter: true,
  headerTemplate: '<span></span>',
  footerTemplate: `<div style="font-size:8px;width:100%;text-align:center;color:#888;">${title} · <span class="pageNumber"></span></div>`,
  margin: { top: '16mm', bottom: '18mm', left: '14mm', right: '14mm' },
});
await browser.close();
