// Draws each table's chart from the table itself, so the page still works with JavaScript off.
// Plot adds its own style element, which the page's policy forbids, so style.css has those rules.
const number = (n) => n.toLocaleString();

function columnChart(table, all, rows, width) {
  const unit = table.dataset.unit;
  const domain = all.map((r) => r.label);
  return {
    height: 260,
    marginLeft: 56,
    x: {
      label: null,
      domain,
      tickFormat: (d, i) => (domain.length > 16 && i % 5 ? "" : d),
    },
    y: { grid: true, label: table.dataset.axis, tickFormat: "s" },
    marks: [
      Plot.barY(rows, {
        x: "label",
        y: "value",
        href: "href",
        target: "_self",
        fill: "var(--chart)",
        inset: 1,
        rx1y2: 3,
        rx2y2: 3,
      }),
      Plot.ruleY([0]),
      Plot.tip(
        rows,
        Plot.pointerX({ x: "label", y: "value", title: (r) => `${r.title}\n${number(r.value)} ${unit}` }),
      ),
    ],
  };
}

function barChart(table, all, rows) {
  const unit = table.dataset.unit;
  const domain = rows
    .slice()
    .sort((a, b) => b.value - a.value)
    .map((r) => r.label);
  return {
    height: rows.length * 34 + 48,
    marginLeft: 96,
    marginRight: 84,
    y: { label: null, domain },
    x: { grid: true, label: table.dataset.axis },
    marks: [
      Plot.barX(rows, {
        y: "label",
        x: "value",
        href: "href",
        target: "_self",
        fill: "var(--chart)",
        insetTop: 4,
        insetBottom: 4,
        rx2y1: 3,
        rx2y2: 3,
      }),
      Plot.text(rows, {
        y: "label",
        x: "value",
        text: (r) => `${number(r.value)} ${unit}`,
        textAnchor: "start",
        dx: 6,
      }),
      Plot.ruleX([0]),
    ],
  };
}

const charts = {
  steps: columnChart,
  "distance-time": columnChart,
  "activity-time": columnChart,
  "distance-type": barChart,
};

// Choosing a filter applies it straight away, and empty filters stay out of the URL.
const form = document.querySelector("form.filters");
if (form) {
  form.classList.add("auto");
  form.addEventListener("change", () => {
    for (const select of form.querySelectorAll("select")) {
      if (select.value === "") select.disabled = true;
    }
    form.requestSubmit();
  });
  // The browser may restore this page from its back cache with those filters still disabled.
  window.addEventListener("pageshow", () => {
    const year = form.querySelector('select[name="year"]');
    for (const select of form.querySelectorAll("select")) {
      select.disabled = select.name === "month" && year.value === "";
    }
  });
}

for (const table of document.querySelectorAll("table[data-chart]")) {
  const all = [...table.querySelectorAll("tr[data-x]")].map((row) => ({
    label: row.dataset.x,
    title: row.dataset.title,
    value: row.dataset.y === undefined ? undefined : Number(row.dataset.y),
    href: row.querySelector("a")?.href,
  }));
  const rows = all.filter((r) => r.value !== undefined);
  if (rows.length === 0) continue;
  const details = table.closest("details");
  const width = details.parentElement.clientWidth;
  const svg = Plot.plot({
    className: "phv-plot",
    width,
    ...charts[table.dataset.chart](table, all, rows, width),
  });
  svg.querySelector("style")?.remove();
  details.before(svg);
  details.open = false;
}
