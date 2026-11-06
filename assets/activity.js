// Draws each table's chart from the table itself, so the page still works with JavaScript off.
// Plot adds its own style element, which the page's policy forbids, so style.css has those rules.
const number = (n) => n.toLocaleString();

// Past this many bars, each is too thin to see or click, so the chart draws a line.
const MOST_BARS = 150;

function columnChart(table, all, rows, width) {
  const unit = table.dataset.unit;
  const domain = all.map((r) => r.label);
  const everyNth = Math.ceil(domain.length / Math.max(1, Math.floor((width - 80) / 72)));
  const tickFormat = (d, i) => (i % everyNth ? "" : d);
  const y = { grid: true, label: table.dataset.axis, tickFormat: "s" };
  const tip = Plot.tip(
    rows,
    Plot.pointerX({ x: "label", y: "value", title: (r) => `${r.title}\n${number(r.value)} ${unit}` }),
  );
  if (domain.length > MOST_BARS) {
    return {
      height: 260,
      marginLeft: 56,
      x: { label: null, type: "point", domain, tickFormat, tickSize: 0 },
      y,
      marks: [
        Plot.ruleY([0]),
        Plot.lineY(
          all.map((r) => ({ ...r, value: r.value ?? NaN })),
          { x: "label", y: "value", stroke: "var(--chart)", strokeWidth: 1.5 },
        ),
        tip,
      ],
    };
  }
  const thin = domain.length > 40;
  return {
    height: 260,
    marginLeft: 56,
    x: { label: null, domain, tickFormat, tickSize: thin ? 0 : 6 },
    y,
    marks: [
      Plot.barY(rows, {
        x: "label",
        y: "value",
        href: "href",
        target: "_self",
        fill: "var(--chart)",
        inset: thin ? 0.5 : 1,
        rx1y2: thin ? 0 : 3,
        rx2y2: thin ? 0 : 3,
      }),
      Plot.ruleY([0]),
      tip,
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

// A GitHub-style calendar: one row of weeks per year, with an empty square for a day with no record.
function calendarChart(table, all, rows, width) {
  const utc = (text) => new Date(`${text}T00:00:00Z`);
  const place = (date) => ({
    date,
    year: String(date.getUTCFullYear()),
    week: d3.utcSunday.count(d3.utcYear(date), date),
    day: date.getUTCDay(),
  });
  const sorted = rows.map((r) => r.value).sort((a, b) => a - b);
  const cuts = [0.25, 0.5, 0.75].map((q) => d3.quantileSorted(sorted, q));
  const level = (value) => 1 + cuts.filter((cut) => value > cut).length;
  const records = rows.map((r) => ({
    ...place(utc(r.label)),
    title: r.title,
    href: r.href,
    fill: `var(--heat-${level(r.value)})`,
  }));
  const years = d3.range(
    d3.min(records, (r) => r.date.getUTCFullYear()),
    d3.max(records, (r) => r.date.getUTCFullYear()) + 1,
  );
  const everyDay = years.flatMap((year) =>
    d3.utcDays(new Date(Date.UTC(year, 0, 1)), new Date(Date.UTC(year + 1, 0, 1))).map(place),
  );
  const firstOfMonth = years.flatMap((year) =>
    d3.utcMonths(new Date(Date.UTC(year, 0, 1)), new Date(Date.UTC(year + 1, 0, 1))).map(place),
  );
  const marginLeft = 24;
  const marginRight = 44;
  // Squares stay readable on a phone, where the calendar scrolls sideways instead.
  const chartWidth = Math.max(width, 620);
  const square = (chartWidth - marginLeft - marginRight) / 54;
  const weekdays = ["", "M", "", "W", "", "F", ""];
  return {
    width: chartWidth,
    height: years.length * (square * 8 + 10) + 14,
    marginLeft,
    marginRight,
    marginTop: 6,
    marginBottom: 6,
    padding: 0,
    color: { type: "identity" },
    x: { axis: null, domain: d3.range(54) },
    y: {
      label: null,
      domain: [-1, 0, 1, 2, 3, 4, 5, 6],
      tickSize: 0,
      tickFormat: (d) => weekdays[d] ?? "",
    },
    fy: { label: null, domain: years.map(String).reverse(), tickSize: 0 },
    marks: [
      Plot.cell(everyDay, {
        x: "week",
        y: "day",
        fy: "year",
        fill: "var(--heat-0)",
        inset: 1,
        rx: 2,
      }),
      Plot.cell(records, {
        x: "week",
        y: "day",
        fy: "year",
        fill: "fill",
        href: "href",
        target: "_self",
        inset: 1,
        rx: 2,
      }),
      Plot.text(firstOfMonth, {
        x: "week",
        fy: "year",
        y: () => -1,
        text: (d) => d3.utcFormat("%b")(d.date),
        textAnchor: "start",
        fontSize: 9,
      }),
      Plot.tip(records, Plot.pointer({ x: "week", y: "day", fy: "year", title: "title" })),
    ],
  };
}

const charts = {
  calendar: calendarChart,
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
  if (table.dataset.chart === "calendar") {
    const scroller = document.createElement("div");
    scroller.className = "calendar-scroll";
    scroller.append(svg);
    details.before(scroller);
    scroller.scrollLeft = scroller.scrollWidth;
  } else {
    details.before(svg);
  }
  details.open = false;
  const legend = details.previousElementSibling.previousElementSibling;
  if (legend?.classList.contains("heat-legend")) legend.hidden = false;
}
