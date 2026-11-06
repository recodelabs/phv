//! The Claims topic: what was billed, what the plan paid and what fell to the patient.

use super::documents::Chip;
use super::explorer::patient_name;
use super::observation::is_entered_in_error;
use super::value::{
    Dated, code_in_system, concept_name, date_or_none, encode_segment, id_of, newest_first,
};
use crate::import::x12_835::GROUP_CODE;
use serde_json::Value;

const DATA_ABSENT: &str = "http://terminology.hl7.org/CodeSystem/data-absent-reason";
const CPT: &str = "http://www.ama-assn.org/go/cpt";
const HCPCS: &str = "https://www.cms.gov/Medicare/Coding/HCPCSReleaseCodeSets";
const REVENUE: &str = "https://www.nubc.org/CodeSystem/RevenueCodes";
const NOT_RECORDED: &str = "Not recorded";
/// How many services a row names before it says how many more there are.
const SERVICES_NAMED: usize = 2;

/// `$1,959.75`, from whole cents.
pub fn dollars(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("{sign}${grouped}.{:02}", abs % 100)
}

fn money_text(cents: Option<i64>) -> String {
    cents.map_or_else(|| NOT_RECORDED.to_string(), dollars)
}

/// Whole cents, or `None` for anything finer, which is never rounded into a total.
fn cents(value: &Value) -> Option<i64> {
    let text = value.as_number()?.to_string();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if fraction.len() > 2 {
        return None;
    }
    let whole: i64 = whole.parse().ok()?;
    let fraction: i64 = format!("{fraction:0<2}").parse().ok()?;
    let amount = whole.checked_mul(100)?.checked_add(fraction)?;
    Some(if negative { -amount } else { amount })
}

/// A name sent in all capitals, as billing files do, in ordinary case.
pub fn tidy_name(name: &str) -> String {
    if name.chars().any(char::is_lowercase) {
        return name.to_string();
    }
    name.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_string() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_category(adjudication: &Value, code: &str) -> bool {
    adjudication["category"]["coding"]
        .as_array()
        .is_some_and(|codings| codings.iter().any(|c| c["code"] == code))
}

fn in_group(adjudication: &Value, group: &str) -> bool {
    adjudication["category"]["coding"]
        .as_array()
        .is_some_and(|codings| {
            codings
                .iter()
                .any(|c| c["system"] == GROUP_CODE && c["code"] == group)
        })
}

fn sum<'a>(adjudications: impl Iterator<Item = &'a Value>) -> Option<i64> {
    let mut total = None;
    for adjudication in adjudications {
        total = Some(total.unwrap_or(0) + cents(&adjudication["amount"]["value"])?);
    }
    total
}

fn adjudications(holder: &Value) -> &[Value] {
    holder["adjudication"].as_array().map_or(&[], Vec::as_slice)
}

fn category_amount(holder: &Value, code: &str) -> Option<i64> {
    sum(adjudications(holder)
        .iter()
        .filter(|a| has_category(a, code)))
}

const PATIENT_PARTS: [(&str, &str); 3] = [
    ("deductible", "Deductible"),
    ("coinsurance", "Coinsurance"),
    ("copay", "Copay"),
];

/// What one line left to the patient. A group code from an 835 says so outright. Otherwise a
/// stated total wins over its parts, because records from other sources may hold both.
fn line_patient(item: &Value) -> Option<i64> {
    sum(adjudications(item).iter().filter(|a| in_group(a, "PR")))
        .or_else(|| category_amount(item, "memberliability"))
        .or_else(|| {
            sum(adjudications(item)
                .iter()
                .filter(|a| PATIENT_PARTS.iter().any(|(code, _)| has_category(a, code))))
        })
}

fn items(claim: &Value) -> &[Value] {
    claim["item"].as_array().map_or(&[], Vec::as_slice)
}

fn stated_total(claim: &Value, code: &str) -> Option<i64> {
    claim["total"]
        .as_array()?
        .iter()
        .find(|t| has_category(t, code))
        .and_then(|t| cents(&t["amount"]["value"]))
}

fn sum_lines(claim: &Value, line: impl Fn(&Value) -> Option<i64>) -> Option<i64> {
    let mut total = None;
    for item in items(claim) {
        if let Some(amount) = line(item) {
            total = Some(total.unwrap_or(0) + amount);
        }
    }
    total
}

/// How one charge, or many, divided up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Split {
    pub billed: Option<i64>,
    pub plan_paid: Option<i64>,
    pub patient: Option<i64>,
    pub discount: Option<i64>,
}

impl Split {
    fn of(claim: &Value) -> Self {
        let total_or_lines = |code: &str| {
            stated_total(claim, code).or_else(|| sum_lines(claim, |i| category_amount(i, code)))
        };
        Self {
            billed: total_or_lines("submitted"),
            plan_paid: total_or_lines("benefit")
                .or_else(|| cents(&claim["payment"]["amount"]["value"])),
            patient: stated_total(claim, "memberliability")
                .or_else(|| sum_lines(claim, line_patient)),
            discount: sum_lines(claim, |i| category_amount(i, "discount")),
        }
    }

    /// Adjustments that are neither the plan's, the patient's nor a discount.
    fn other(self) -> i64 {
        self.billed.unwrap_or(0)
            - self.plan_paid.unwrap_or(0)
            - self.patient.unwrap_or(0)
            - self.discount.unwrap_or(0)
    }

    fn add(self, other: Split) -> Split {
        let plus = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (None, None) => None,
            _ => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };
        Split {
            billed: plus(self.billed, other.billed),
            plan_paid: plus(self.plan_paid, other.plan_paid),
            patient: plus(self.patient, other.patient),
            discount: plus(self.discount, other.discount),
        }
    }
}

/// One slice of the bar, which is also the tile that says what the slice is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub label: &'static str,
    pub class: &'static str,
    pub note: &'static str,
    pub amount: String,
    pub percent: String,
    /// Where the slice starts and how wide it is, as SVG percentages.
    pub x: String,
    pub width: String,
    pub drawn: bool,
}

/// Slices in a fixed order, so a color always means one thing. No slices are drawn if the
/// amounts cannot make a whole, such as a missing total or a negative part.
pub fn segments(split: Split) -> Vec<Segment> {
    let parts: [(&str, &str, &str, Option<i64>); 4] = [
        (
            "Plan paid",
            "split-plan",
            "Paid to the provider by the plan",
            split.plan_paid,
        ),
        (
            "Patient's share",
            "split-patient",
            "Deductible, coinsurance and copays. The records don't show whether it has been paid.",
            split.patient,
        ),
        (
            "Discount",
            "split-discount",
            "Taken off under the provider's contract with the plan",
            split.discount,
        ),
        (
            "Other adjustments",
            "split-other",
            "Taken off for other reasons",
            Some(split.other()).filter(|other| *other != 0),
        ),
    ];
    let billed = split.billed.filter(|b| *b > 0);
    let drawable = billed.is_some() && parts.iter().all(|p| p.3.unwrap_or(0) >= 0);
    let mut start = 0i64;
    parts
        .into_iter()
        .filter_map(|(label, class, note, amount)| {
            let amount = amount?;
            let share = |value: i64| value as f64 * 100.0 / billed.unwrap_or(1) as f64;
            let segment = Segment {
                label,
                class,
                note,
                amount: dollars(amount),
                percent: if drawable {
                    format!("{:.0}%", share(amount))
                } else {
                    String::new()
                },
                x: format!("{:.2}%", share(start)),
                width: format!("{:.2}%", share(amount)),
                drawn: drawable && amount > 0,
            };
            start += amount;
            Some(segment)
        })
        .collect()
}

/// Whether there is a bar to draw, since a template cannot ask this itself.
pub fn any_drawn(segments: &[Segment]) -> bool {
    segments.iter().any(|s| s.drawn)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRow {
    pub id: String,
    pub id_url: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub provider: String,
    pub kind: Option<String>,
    pub services: String,
    pub billed: String,
    pub plan_paid: String,
    pub patient: String,
    pub note: Option<&'static str>,
    pub split: Split,
}

impl Dated for ClaimRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

fn find_reference<'a>(holder: &Value, resources: &'a [Value]) -> Option<&'a Value> {
    let reference = holder["reference"].as_str()?;
    let (_, id) = reference.split_once('/')?;
    resources.iter().find(|r| r["id"] == id)
}

/// The stored organization's name, otherwise the name written on the claim.
fn organization_name(holder: &Value, organizations: &[Value]) -> Option<String> {
    find_reference(holder, organizations)
        .and_then(|o| o["name"].as_str())
        .or(holder["display"].as_str())
        .map(tidy_name)
}

/// Plain descriptions in our own words of codes that appear on claims. Any other code
/// shows as itself, and the official code is always shown next to the description.
fn procedure_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "96156" => "Health behavior assessment",
        "99213" => "Office visit, established patient",
        "99283" => "Emergency room visit",
        "99396" => "Yearly preventive checkup",
        "99495" => "Follow-up care after a hospital stay",
        _ => return None,
    })
}

fn revenue_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "0300" => "Laboratory",
        "0450" => "Emergency room",
        "0510" => "Clinic",
        "0771" => "Vaccine administration",
        "0914" => "Behavioral health therapy",
        _ => return None,
    })
}

/// A plain name for a line's service, and the official code it came from.
fn service_name(item: &Value) -> (String, String) {
    let concept = &item["productOrService"];
    let not_applicable = code_in_system(concept, DATA_ABSENT).is_some();
    let revenue = code_in_system(&item["revenue"], REVENUE);
    if not_applicable && let Some(code) = &revenue {
        let name =
            revenue_name(code).map_or_else(|| "Hospital service".to_string(), str::to_string);
        return (name, format!("Revenue code {code}"));
    }
    let cpt = code_in_system(concept, CPT);
    let hcpcs = code_in_system(concept, HCPCS);
    let code = cpt.as_deref().or(hcpcs.as_deref());
    let name = concept["text"]
        .as_str()
        .or(concept["coding"][0]["display"].as_str())
        .or(code.and_then(procedure_name))
        .map(str::to_string);
    let code_text = match (&cpt, &hcpcs) {
        (Some(code), _) => format!("CPT {code}"),
        (_, Some(code)) => format!("HCPCS {code}"),
        _ => concept["coding"][0]["code"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    };
    let name = name
        .or_else(|| concept_name(concept))
        .unwrap_or_else(|| "Service".to_string());
    (name, code_text)
}

fn services_summary(claim: &Value) -> String {
    let mut names: Vec<String> = Vec::new();
    for item in items(claim) {
        let (name, _) = service_name(item);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let more = names.len().saturating_sub(SERVICES_NAMED);
    names.truncate(SERVICES_NAMED);
    let mut text = names.join(", ");
    if more > 0 {
        text.push_str(&format!(", and {more} more"));
    }
    text
}

fn place_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "02" | "10" => "Telehealth",
        "11" => "Office",
        "12" => "Home",
        "20" => "Urgent care",
        "21" => "Hospital inpatient",
        "22" => "Hospital outpatient",
        "23" => "Emergency room",
        _ => return None,
    })
}

/// Where the care happened, from the place of service or the hospital's type of bill.
fn claim_kind(claim: &Value) -> Option<String> {
    let bill = claim["supportingInfo"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| has_category(s, "typeofbill"))
        .and_then(|s| s["code"]["coding"][0]["code"].as_str());
    if let Some(bill) = bill {
        return Some(
            match bill.trim_start_matches('0').get(..2) {
                Some("11") => "Hospital inpatient",
                Some("13") => "Hospital outpatient",
                _ => "Facility",
            }
            .to_string(),
        );
    }
    items(claim)
        .iter()
        .find_map(|i| i["locationCodeableConcept"]["coding"][0]["code"].as_str())
        .and_then(place_name)
        .map(str::to_string)
}

fn date_of(claim: &Value) -> Option<String> {
    claim["billablePeriod"]["start"]
        .as_str()
        .or(claim["created"].as_str())
        .map(str::to_string)
}

fn claim_row(claim: &Value, organizations: &[Value]) -> ClaimRow {
    let id = id_of(claim);
    let raw_date = date_of(claim);
    let split = Split::of(claim);
    let deductible = sum_lines(claim, |i| category_amount(i, "deductible")).unwrap_or(0);
    ClaimRow {
        id_url: encode_segment(&id),
        id,
        date: date_or_none(raw_date.as_deref()),
        raw_date,
        provider: organization_name(&claim["provider"], organizations)
            .unwrap_or_else(|| "Unknown provider".to_string()),
        kind: claim_kind(claim),
        services: services_summary(claim),
        billed: money_text(split.billed),
        plan_paid: money_text(split.plan_paid),
        patient: money_text(split.patient),
        note: (split.plan_paid == Some(0) && deductible > 0)
            .then_some("Went toward the deductible"),
        split,
    }
}

/// Every claim, newest first, leaving out those entered in error.
pub fn claim_rows(claims: &[Value], organizations: &[Value]) -> Vec<ClaimRow> {
    let mut rows: Vec<ClaimRow> = claims
        .iter()
        .filter(|c| !is_entered_in_error(c))
        .map(|c| claim_row(c, organizations))
        .collect();
    newest_first(&mut rows);
    rows
}

fn year_of(row: &ClaimRow) -> Option<&str> {
    row.raw_date.as_deref()?.get(..4)
}

pub fn in_year(row: &ClaimRow, year: &str) -> bool {
    year_of(row) == Some(year)
}

/// `All`, then one chip for each year, newest first.
pub fn year_chips(rows: &[ClaimRow], selected: Option<&str>) -> Vec<Chip> {
    let mut years: Vec<&str> = rows.iter().filter_map(year_of).collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    let mut chips = vec![Chip {
        code: String::new(),
        label: "All".to_string(),
        count: rows.len(),
        on: selected.is_none(),
    }];
    chips.extend(years.into_iter().map(|year| Chip {
        code: year.to_string(),
        label: year.to_string(),
        count: rows.iter().filter(|r| in_year(r, year)).count(),
        on: selected == Some(year),
    }));
    chips
}

pub fn total_split(rows: &[ClaimRow]) -> Split {
    rows.iter()
        .fold(Split::default(), |total, row| total.add(row.split))
}

pub fn billed_text(split: Split) -> String {
    money_text(split.billed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLine {
    pub date: String,
    pub name: String,
    pub code: String,
    pub billed: String,
    pub plan_paid: String,
    pub discount: String,
    pub patient: String,
    /// Which parts the patient's share is made of, such as `Coinsurance $29.24`.
    pub patient_parts: Vec<String>,
}

fn service_line(claim: &Value, item: &Value) -> ServiceLine {
    let (name, code) = service_name(item);
    let date = item["servicedDate"]
        .as_str()
        .or(item["servicedPeriod"]["start"].as_str())
        .or(claim["billablePeriod"]["start"].as_str());
    let amount = |code: &str| money_text(category_amount(item, code));
    ServiceLine {
        date: date_or_none(date),
        name,
        code,
        billed: amount("submitted"),
        plan_paid: amount("benefit"),
        discount: category_amount(item, "discount").map_or_else(|| "$0.00".to_string(), dollars),
        patient: money_text(line_patient(item)),
        patient_parts: PATIENT_PARTS
            .iter()
            .filter_map(|(code, label)| {
                let amount = category_amount(item, code)?;
                Some(format!("{label} {}", dollars(amount)))
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub label: &'static str,
    pub value: String,
}

/// What the claim says apart from the money, in the order someone would ask.
fn facts(
    claim: &Value,
    coverage: &[Value],
    organizations: &[Value],
    practitioners: &[Value],
) -> Vec<Fact> {
    let info = |code: &str| {
        claim["supportingInfo"]
            .as_array()?
            .iter()
            .find(|s| has_category(s, code))
    };
    let text = |value: Option<&str>| value.map(str::to_string);
    let friendly = |value: Option<&str>| value.map(|d| date_or_none(Some(d)));
    let doctor = claim["careTeam"][0]["provider"].clone();
    let rows = [
        (
            "Insurance plan",
            organization_name(&claim["insurer"], organizations),
        ),
        (
            "Member ID",
            find_reference(&claim["insurance"][0]["coverage"], coverage)
                .and_then(|c| c["subscriberId"].as_str())
                .map(str::to_string),
        ),
        (
            "Treated by",
            find_reference(&doctor, practitioners)
                .and_then(patient_name)
                .or_else(|| text(doctor["display"].as_str())),
        ),
        (
            "Claim number",
            text(claim["identifier"][0]["value"].as_str()),
        ),
        (
            "Claim received",
            friendly(info("clmrecvddate").and_then(|s| s["timingDate"].as_str())),
        ),
        ("Paid on", friendly(claim["payment"]["date"].as_str())),
        (
            "Payment reference",
            text(claim["payment"]["identifier"]["value"].as_str()),
        ),
        (
            "Account number",
            text(info("patientaccountnumber").and_then(|s| s["valueString"].as_str())),
        ),
        (
            "Medical record",
            text(info("medicalrecordnumber").and_then(|s| s["valueString"].as_str())),
        ),
    ];
    rows.into_iter()
        .filter_map(|(label, value)| {
            Some(Fact {
                label,
                value: value?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimDetail {
    pub row: ClaimRow,
    pub segments: Vec<Segment>,
    pub lines: Vec<ServiceLine>,
    pub facts: Vec<Fact>,
}

pub fn claim_detail(
    id: &str,
    claims: &[Value],
    coverage: &[Value],
    organizations: &[Value],
    practitioners: &[Value],
) -> Option<ClaimDetail> {
    let claim = claims
        .iter()
        .find(|c| c["id"] == id && !is_entered_in_error(c))?;
    let row = claim_row(claim, organizations);
    Some(ClaimDetail {
        segments: segments(row.split),
        lines: items(claim)
            .iter()
            .map(|i| service_line(claim, i))
            .collect(),
        facts: facts(claim, coverage, organizations, practitioners),
        row,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slices_add_up_to_the_whole_bar_and_keep_their_order() {
        let split = Split {
            billed: Some(10000),
            plan_paid: Some(2500),
            patient: Some(2500),
            discount: Some(5000),
        };
        let slices = segments(split);
        assert_eq!(
            slices.iter().map(|s| s.label).collect::<Vec<_>>(),
            ["Plan paid", "Patient's share", "Discount"]
        );
        assert_eq!(slices[2].x, "50.00%");
        assert_eq!(slices[2].width, "50.00%");
        assert_eq!(slices[0].percent, "25%");
    }

    #[test]
    fn anything_left_over_is_shown_rather_than_hidden() {
        let split = Split {
            billed: Some(10000),
            plan_paid: Some(2500),
            patient: Some(2500),
            discount: Some(4000),
        };
        let other = segments(split).pop().unwrap();
        assert_eq!(other.label, "Other adjustments");
        assert_eq!(other.amount, "$10.00");
    }

    #[test]
    fn a_split_that_cannot_make_a_whole_draws_no_bar() {
        let split = Split {
            billed: None,
            plan_paid: Some(100),
            patient: None,
            discount: None,
        };
        assert!(
            segments(split)
                .iter()
                .all(|s| !s.drawn && s.percent.is_empty())
        );
        let refund = Split {
            billed: Some(100),
            plan_paid: Some(-100),
            patient: Some(200),
            discount: None,
        };
        assert!(segments(refund).iter().all(|s| !s.drawn));
    }

    #[test]
    fn amounts_finer_than_a_cent_are_not_rounded() {
        assert_eq!(cents(&json!(12)), Some(1200));
        assert_eq!(cents(&json!(12.5)), Some(1250));
        assert_eq!(cents(&json!(12.345)), None);
        assert_eq!(cents(&json!("12.00")), None);
    }

    #[test]
    fn a_patient_total_is_not_counted_twice_with_its_parts() {
        let item = json!({"adjudication":[
            {"category":{"coding":[{"code":"memberliability"}]},"amount":{"value":30}},
            {"category":{"coding":[{"code":"copay"}]},"amount":{"value":30}}
        ]});
        assert_eq!(line_patient(&item), Some(3000));
    }
}
