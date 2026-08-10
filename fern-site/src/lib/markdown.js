/**
 * 更新日志那一小撮 Markdown。
 *
 * 渲染的东西来路是确定的：`CHANGELOG.md` 里的一节，由 `.github/build-manifest.py`
 * 原样搬进清单的 `notes`。那一节的写法在 AGENTS.md 里定死了——`### 新增` 这样的
 * 小标题，底下一列 `- ` 条目。所以这里只认标题、列表、段落，加上行内的强调、代码
 * 和链接。
 *
 * 为这点东西装一个完整的 Markdown 库，是拿通用解析器去解一份自己写的、发版时还会
 * 被检查一遍的文本，而通用解析器还得再配一层消毒。这里反过来做：**先把整段文本
 * 转义，再往里放自己生成的标签**。没认出来的记号最坏也只是原样显示出来，不会变成
 * 标签——清单是从网上读回来的，这一条是安全边界，不只是稳妥。
 */

const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };

const escape = (text) => text.replace(/[&<>"]/g, (c) => ESCAPES[c]);

/* 中日韩文字和全角标点。 */
const CJK = /[⺀-鿿　-〿＀-￯]/;

/**
 * 段落里的换行。
 *
 * Markdown 把它当一个空格，那是给西文定的规矩；一段中文在这儿断行，接回去会多出
 * 一个看得见的空格。所以两头都是中日韩字符时直接接上，其余照旧留空格。
 */
function unwrap(lines) {
  return lines.reduce((left, right) => {
    if (!left) return right;
    return CJK.test(left.at(-1)) && CJK.test(right[0]) ? left + right : `${left} ${right}`;
  }, '');
}

/** 行内记号。代码先切出来——代码里的星号不是强调。 */
function inline(text) {
  return text
    .split(/(`[^`]+`)/)
    .map((part) => {
      if (part.length > 1 && part.startsWith('`') && part.endsWith('`')) {
        return `<code>${escape(part.slice(1, -1))}</code>`;
      }
      return escape(part)
        .replace(
          /\[([^\]]+)\]\((https?:\/\/[^\s)"']+)\)/g,
          '<a href="$2" target="_blank" rel="noreferrer">$1</a>'
        )
        .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    })
    .join('');
}

/** 一段更新日志 → 一段 HTML。给不出内容就是空串，调用方据此决定要不要留位置。 */
export function renderNotes(source) {
  const lines = String(source ?? '')
    .replace(/\r\n?/g, '\n')
    .split('\n');

  const out = [];
  let para = [];
  let list = null;

  const closePara = () => {
    if (!para.length) return;
    out.push(`<p>${inline(unwrap(para))}</p>`);
    para = [];
  };
  const closeList = () => {
    if (!list) return;
    const items = list.items.map((item) => `<li>${inline(unwrap(item))}</li>`).join('');
    out.push(`<${list.tag}>${items}</${list.tag}>`);
    list = null;
  };
  const close = () => {
    closePara();
    closeList();
  };

  for (const raw of lines) {
    const line = raw.trim();
    if (!line) {
      close();
      continue;
    }

    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading) {
      close();
      /* 页面自己占着 h1 和 h2，日志里的标题从 h3 往下排。 */
      const level = Math.min(5, Math.max(3, heading[1].length));
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      continue;
    }

    const bullet = /^([-*+]|\d{1,9}[.)])\s+(.+)$/.exec(line);
    if (bullet) {
      const tag = /^\d/.test(bullet[1]) ? 'ol' : 'ul';
      closePara();
      /* 中途换了记号就是换了一列，不要把两列并成一列。 */
      if (list?.tag !== tag) closeList();
      list ??= { tag, items: [] };
      list.items.push([bullet[2]]);
      continue;
    }

    /* 缩进的续行接着上一条写，不另起一段。 */
    if (list && /^\s/.test(raw)) {
      list.items.at(-1).push(line);
      continue;
    }

    closeList();
    para.push(line);
  }

  close();
  return out.join('');
}
