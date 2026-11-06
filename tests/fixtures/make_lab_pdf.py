#!/usr/bin/env python3
"""Write a minimal synthetic lab report PDF for trying the skill. No real health data."""
import sys

LINES = [
    "Example Labs - Laboratory Report",
    "Patient: Test Example    DOB: 1980-01-01",
    "Collected: 2026-03-14",
    "Basic metabolic panel",
    "Test              Result    Units     Reference range",
    "Creatinine        1.50 H    mg/dL     0.70 - 1.30",
    "Sodium            140       mmol/L    135 - 145",
    "Glucose           <50 L     mg/dL     70 - 99",
]


def main(path):
    text = "BT /F1 11 Tf 50 740 Td 16 TL "
    for line in LINES:
        text += "(" + line.replace("(", r"\(").replace(")", r"\)") + ") Tj T* "
    text += "ET"
    objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R "
        "/Resources << /Font << /F1 5 0 R >> >> >>",
        f"<< /Length {len(text)} >>\nstream\n{text}\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
    ]
    out = "%PDF-1.4\n"
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n{body}\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n"
    out += "".join(f"{offset:010d} 00000 n \n" for offset in offsets)
    out += f"trailer << /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    with open(path, "w", encoding="latin-1") as handle:
        handle.write(out)


if __name__ == "__main__":
    main(sys.argv[1])
