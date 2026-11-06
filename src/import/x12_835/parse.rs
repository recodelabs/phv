//! Reads an X12 835 file into plain data and checks that it is internally consistent.
//!
//! This fails closed. A segment it does not understand refuses the file, because losing a
//! remark code or a payer adjustment would quietly change what a claim means.

use chrono::NaiveDate;
use std::collections::BTreeSet;

/// Money as whole cents, so that adding amounts up is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cents(pub i64);

impl Cents {
    fn parse(text: &str) -> Result<Self, String> {
        let bad = || format!("{text:?} is not an amount");
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let numeric = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty() && fraction.is_empty()
            || !numeric(whole)
            || !numeric(fraction)
            || fraction.len() > 2
        {
            return Err(bad());
        }
        let whole: i64 = if whole.is_empty() {
            0
        } else {
            whole.parse().map_err(|_| bad())?
        };
        let fraction: i64 = format!("{fraction:0<2}").parse().map_err(|_| bad())?;
        let cents = whole
            .checked_mul(100)
            .and_then(|c| c.checked_add(fraction))
            .ok_or_else(bad)?;
        Ok(Self(if negative { -cents } else { cents }))
    }

    /// Two decimal places, as a FHIR decimal.
    pub fn text(self) -> String {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        format!("{sign}{}.{:02}", abs / 100, abs % 100)
    }
}

impl std::iter::Sum for Cents {
    fn sum<I: Iterator<Item = Cents>>(iter: I) -> Self {
        Self(iter.map(|c| c.0).sum())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Party {
    pub name: String,
    /// NPI for a provider.
    pub npi: Option<String>,
    pub tax_id: Option<String>,
    pub line: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjustment {
    /// CAS01: CO, PR, OA, PI or CR.
    pub group: String,
    /// The Claim Adjustment Reason Code.
    pub reason: String,
    pub amount: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// SVC01-1: HC for a procedure code, NU for a revenue code.
    pub qualifier: String,
    pub code: String,
    pub modifiers: Vec<String>,
    pub charge: Cents,
    pub paid: Cents,
    /// SVC04, a revenue code alongside a procedure code.
    pub revenue: Option<String>,
    /// SVC05. An empty element means one unit.
    pub units: Option<String>,
    pub date: Option<NaiveDate>,
    pub allowed: Option<Cents>,
    pub adjustments: Vec<Adjustment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub family: String,
    pub given: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// CLP01, the provider's account number for the patient.
    pub patient_control: String,
    pub charge: Cents,
    pub paid: Cents,
    pub patient_owes: Cents,
    /// CLP06.
    pub filing_indicator: String,
    /// CLP07, the payer's claim control number.
    pub control_number: String,
    /// CLP08: place of service, or the start of a type of bill.
    pub facility: String,
    /// CLP09.
    pub frequency: String,
    /// True when a TS3 summary says this is a facility bill.
    pub institutional: bool,
    pub patient: Person,
    pub rendering: Option<Person>,
    pub medical_record: Option<String>,
    pub statement_start: Option<NaiveDate>,
    pub statement_end: Option<NaiveDate>,
    pub received: Option<NaiveDate>,
    pub coverage_amount: Option<Cents>,
    pub services: Vec<Service>,
}

/// One ST..SE: a payment or notice from one payer to one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remittance {
    pub total: Cents,
    pub payment_date: NaiveDate,
    pub produced: Option<NaiveDate>,
    pub trace: String,
    pub payer_id: String,
    pub payer: Party,
    pub payee: Party,
    pub claims: Vec<Claim>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interchange {
    /// ISA15 is `T`.
    pub marked_as_test: bool,
    pub remittances: Vec<Remittance>,
}

/// Claim statuses this importer can represent. A reversal needs the original claim marked,
/// and a secondary claim needs a second payer, so neither is guessed at.
const PROCESSED_AS_PRIMARY: &str = "1";

struct Segment<'a> {
    id: &'a str,
    elements: Vec<&'a str>,
}

impl<'a> Segment<'a> {
    /// X12 numbers elements from 1. A missing element reads as empty.
    fn get(&self, position: usize) -> &'a str {
        self.elements.get(position).copied().unwrap_or("")
    }

    fn required(&self, position: usize) -> Result<&'a str, String> {
        match self.get(position) {
            "" => Err(format!("{}{position:02} is missing", self.id)),
            value => Ok(value),
        }
    }

    fn amount(&self, position: usize) -> Result<Cents, String> {
        Cents::parse(self.required(position)?).map_err(|e| format!("{}{position:02}: {e}", self.id))
    }

    fn date(&self, position: usize) -> Result<NaiveDate, String> {
        let text = self.required(position)?;
        NaiveDate::parse_from_str(text, "%Y%m%d")
            .map_err(|_| format!("{}{position:02}: {text:?} is not a date", self.id))
    }
}

/// The ISA has a fixed length, and its own characters say what the separators are.
fn segments(text: &str) -> Result<(Vec<Segment<'_>>, char), String> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if !text.is_ascii() {
        return Err("this file has characters X12 does not allow".into());
    }
    if !text.starts_with("ISA") || text.len() < 106 {
        return Err("this is not an X12 file: it does not start with a full ISA segment".into());
    }
    let bytes = text.as_bytes();
    let element = bytes[3] as char;
    let component = bytes[104] as char;
    let terminator = bytes[105] as char;
    let segments = text
        .split(terminator)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let elements: Vec<&str> = s.split(element).collect();
            Segment {
                id: elements[0],
                elements,
            }
        })
        .collect();
    Ok((segments, component))
}

fn postal_code(text: &str) -> String {
    match text.len() {
        9 => format!("{}-{}", &text[..5], &text[5..]),
        _ => text.to_string(),
    }
}

fn person(segment: &Segment) -> Result<Person, String> {
    Ok(Person {
        family: segment.required(3)?.to_string(),
        given: segment.get(4).to_string(),
        id: Some(segment.get(9).to_string()).filter(|id| !id.is_empty()),
    })
}

#[derive(Clone, Copy, PartialEq)]
enum PartyKind {
    Payer,
    Payee,
}

/// The transaction being read.
#[derive(Default)]
struct Builder {
    total: Option<Cents>,
    payment_date: Option<NaiveDate>,
    produced: Option<NaiveDate>,
    trace: Option<String>,
    payer_id: Option<String>,
    payer: Party,
    payee: Party,
    party: Option<PartyKind>,
    /// The TS3 facility code of the current LX group.
    facility_summary: Option<String>,
    claims: Vec<Claim>,
    unknown: BTreeSet<String>,
}

impl Builder {
    fn party_mut(&mut self) -> Option<&mut Party> {
        match self.party? {
            PartyKind::Payer => Some(&mut self.payer),
            PartyKind::Payee => Some(&mut self.payee),
        }
    }

    fn claim_mut(&mut self) -> Option<&mut Claim> {
        self.claims.last_mut()
    }

    fn service_mut(&mut self) -> Option<&mut Service> {
        self.claims.last_mut()?.services.last_mut()
    }

    fn unknown(&mut self, segment: &Segment) {
        let qualifier = segment.get(1);
        let known_without_qualifier = ["CAS", "SVC", "PER", "N3", "N4", "LX", "TS3"];
        self.unknown.insert(
            if qualifier.is_empty() || known_without_qualifier.contains(&segment.id) {
                segment.id.to_string()
            } else {
                format!("{}*{qualifier}", segment.id)
            },
        );
    }

    fn read(&mut self, segment: &Segment, component: char) -> Result<(), String> {
        match segment.id {
            "BPR" => {
                if segment.get(3) != "C" {
                    return Err("BPR03: only credits to a provider are supported".into());
                }
                self.total = Some(segment.amount(2)?);
                self.payment_date = Some(segment.date(16)?);
            }
            "TRN" => {
                self.trace = Some(segment.required(2)?.to_string());
                self.payer_id = Some(segment.required(3)?.to_string());
            }
            "N1" => match segment.get(1) {
                "PR" => {
                    self.party = Some(PartyKind::Payer);
                    self.payer.name = segment.required(2)?.to_string();
                }
                "PE" => {
                    self.party = Some(PartyKind::Payee);
                    self.payee.name = segment.required(2)?.to_string();
                    if segment.get(3) != "XX" {
                        return Err("N103: only a provider with an NPI is supported".into());
                    }
                    self.payee.npi = Some(segment.required(4)?.to_string());
                }
                _ => self.unknown(segment),
            },
            "N3" | "N4" if self.party.is_none() => self.unknown(segment),
            "N3" => {
                let line = segment.required(1)?.to_string();
                self.party_mut().expect("checked above").line = Some(line);
            }
            "N4" => {
                let party = self.party_mut().expect("checked above");
                party.city = Some(segment.required(1)?.to_string());
                party.state = Some(segment.get(2).to_string()).filter(|s| !s.is_empty());
                party.postal_code = Some(postal_code(segment.get(3))).filter(|s| !s.is_empty());
            }
            // A payer's contact details say nothing about a claim.
            "PER" => {}
            "LX" => self.facility_summary = None,
            "TS3" => self.facility_summary = Some(segment.required(2)?.to_string()),
            "CLP" => self.read_claim(segment)?,
            "SVC" => self.read_service(segment, component)?,
            "CAS" => self.read_adjustment(segment)?,
            "NM1" => match (segment.get(1), self.claim_mut().is_some()) {
                ("QC", true) => {
                    let patient = person(segment)?;
                    self.claim_mut().expect("checked").patient = patient;
                }
                ("82", true) => {
                    if segment.get(8) != "XX" {
                        return Err(
                            "NM108: only a rendering provider with an NPI is supported".into()
                        );
                    }
                    let rendering = person(segment)?;
                    self.claim_mut().expect("checked").rendering = Some(rendering);
                }
                _ => self.unknown(segment),
            },
            "REF" => match segment.get(1) {
                "TJ" => self.payee.tax_id = Some(segment.required(2)?.to_string()),
                "EA" if self.claim_mut().is_some() => {
                    let record = segment.required(2)?.to_string();
                    self.claim_mut().expect("checked").medical_record = Some(record);
                }
                // A line control number only matters to the provider's billing system.
                "6R" if self.service_mut().is_some() => {}
                _ => self.unknown(segment),
            },
            "AMT" => match segment.get(1) {
                "AU" if self.claim_mut().is_some() => {
                    let amount = segment.amount(2)?;
                    self.claim_mut().expect("checked").coverage_amount = Some(amount);
                }
                "B6" if self.service_mut().is_some() => {
                    let amount = segment.amount(2)?;
                    self.service_mut().expect("checked").allowed = Some(amount);
                }
                _ => self.unknown(segment),
            },
            "DTM" => match segment.get(1) {
                "405" => self.produced = Some(segment.date(2)?),
                "050" | "232" | "233" if self.claim_mut().is_some() => {
                    let date = segment.date(2)?;
                    let claim = self.claim_mut().expect("checked");
                    match segment.get(1) {
                        "050" => claim.received = Some(date),
                        "232" => claim.statement_start = Some(date),
                        _ => claim.statement_end = Some(date),
                    }
                }
                "472" if self.service_mut().is_some() => {
                    let date = segment.date(2)?;
                    self.service_mut().expect("checked").date = Some(date);
                }
                _ => self.unknown(segment),
            },
            _ => self.unknown(segment),
        }
        Ok(())
    }

    fn read_claim(&mut self, segment: &Segment) -> Result<(), String> {
        let control_number = segment.required(7)?.to_string();
        let status = segment.required(2)?;
        if status != PROCESSED_AS_PRIMARY {
            return Err(format!(
                "claim {control_number} has status {status}, and phv only reads claims processed as primary (status 1) so far"
            ));
        }
        let facility = segment.required(8)?.to_string();
        self.claims.push(Claim {
            patient_control: segment.required(1)?.to_string(),
            charge: segment.amount(3)?,
            paid: segment.amount(4)?,
            patient_owes: segment.amount(5)?,
            filing_indicator: segment.required(6)?.to_string(),
            institutional: self.facility_summary.as_deref() == Some(&facility),
            control_number,
            facility,
            frequency: segment.required(9)?.to_string(),
            patient: Person {
                family: String::new(),
                given: String::new(),
                id: None,
            },
            rendering: None,
            medical_record: None,
            statement_start: None,
            statement_end: None,
            received: None,
            coverage_amount: None,
            services: Vec::new(),
        });
        Ok(())
    }

    fn read_service(&mut self, segment: &Segment, component: char) -> Result<(), String> {
        if self.claim_mut().is_none() {
            self.unknown(segment);
            return Ok(());
        }
        let composite = segment.required(1)?;
        let mut parts = composite.split(component);
        let qualifier = parts.next().unwrap_or("").to_string();
        let code = parts.next().unwrap_or("").to_string();
        if !matches!(qualifier.as_str(), "HC" | "NU") || code.is_empty() {
            return Err(format!("SVC01: phv cannot read the service {composite:?}"));
        }
        // An adjudicated code that differs from the billed one needs both shown.
        if !segment.get(6).is_empty() || !segment.get(7).is_empty() {
            return Err(format!(
                "SVC06: the payer changed the billed service {composite:?}, which phv cannot show yet"
            ));
        }
        let service = Service {
            modifiers: parts
                .take(4)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
                .collect(),
            qualifier,
            code,
            charge: segment.amount(2)?,
            paid: segment.amount(3)?,
            revenue: Some(segment.get(4).to_string()).filter(|r| !r.is_empty()),
            units: Some(segment.get(5).to_string()).filter(|u| !u.is_empty()),
            date: None,
            allowed: None,
            adjustments: Vec::new(),
        };
        self.claim_mut().expect("checked").services.push(service);
        Ok(())
    }

    /// CAS01 is the group, then up to six reason, amount and quantity triplets.
    fn read_adjustment(&mut self, segment: &Segment) -> Result<(), String> {
        let Some(service) = self.service_mut() else {
            self.unknown(segment);
            return Ok(());
        };
        let group = segment.required(1)?.to_string();
        let mut position = 2;
        while position + 1 < segment.elements.len() {
            let reason = segment.get(position);
            if !reason.is_empty() {
                service.adjustments.push(Adjustment {
                    group: group.clone(),
                    reason: reason.to_string(),
                    amount: segment.amount(position + 1)?,
                });
            }
            position += 3;
        }
        Ok(())
    }

    fn finish(self) -> Result<Remittance, String> {
        if !self.unknown.is_empty() {
            let names: Vec<String> = self.unknown.into_iter().collect();
            return Err(format!(
                "phv cannot read these segments yet, so it did not import the file: {}",
                names.join(", ")
            ));
        }
        let missing = |name: &str| format!("the {name} segment is missing");
        let remittance = Remittance {
            total: self.total.ok_or_else(|| missing("BPR"))?,
            payment_date: self.payment_date.ok_or_else(|| missing("BPR"))?,
            produced: self.produced,
            trace: self.trace.ok_or_else(|| missing("TRN"))?,
            payer_id: self.payer_id.ok_or_else(|| missing("TRN"))?,
            payer: self.payer,
            payee: self.payee,
            claims: self.claims,
        };
        check(&remittance)?;
        Ok(remittance)
    }
}

/// The balancing rules of the 835 guide, plus what a record cannot do without.
fn check(remittance: &Remittance) -> Result<(), String> {
    if remittance.payer.name.is_empty() {
        return Err("the payer (N1*PR) is missing".into());
    }
    if remittance.payee.name.is_empty() {
        return Err("the provider (N1*PE) is missing".into());
    }
    if remittance.claims.is_empty() {
        return Err("the remittance has no claims".into());
    }
    let unbalanced = |what: String, expected: Cents, actual: Cents| {
        format!(
            "the amounts do not add up: {what} is {} but its parts come to {}",
            expected.text(),
            actual.text()
        )
    };
    for claim in &remittance.claims {
        let name = &claim.control_number;
        if claim.patient.family.is_empty() {
            return Err(format!("claim {name} does not name a patient (NM1*QC)"));
        }
        if claim.patient.id.is_none() {
            return Err(format!("claim {name} has no member ID (NM109)"));
        }
        if claim.services.is_empty() {
            return Err(format!("claim {name} has no service lines"));
        }
        for service in &claim.services {
            let adjusted: Cents = service.adjustments.iter().map(|a| a.amount).sum();
            let parts = Cents(service.paid.0 + adjusted.0);
            if parts != service.charge {
                return Err(unbalanced(
                    format!("the charge for {} on claim {name}", service.code),
                    service.charge,
                    parts,
                ));
            }
            if service.date.is_none() && claim.statement_start.is_none() {
                return Err(format!(
                    "claim {name}: the service {} has no date (DTM*472)",
                    service.code
                ));
            }
        }
        let charged: Cents = claim.services.iter().map(|s| s.charge).sum();
        if charged != claim.charge {
            return Err(unbalanced(
                format!("the charge on claim {name}"),
                claim.charge,
                charged,
            ));
        }
        let paid: Cents = claim.services.iter().map(|s| s.paid).sum();
        if paid != claim.paid {
            return Err(unbalanced(
                format!("the payment on claim {name}"),
                claim.paid,
                paid,
            ));
        }
        let owes: Cents = claim
            .services
            .iter()
            .flat_map(|s| &s.adjustments)
            .filter(|a| a.group == "PR")
            .map(|a| a.amount)
            .sum();
        if owes != claim.patient_owes {
            return Err(unbalanced(
                format!("the patient's share of claim {name}"),
                claim.patient_owes,
                owes,
            ));
        }
    }
    let paid: Cents = remittance.claims.iter().map(|c| c.paid).sum();
    if paid != remittance.total {
        return Err(unbalanced(
            format!("the total of payment {}", remittance.trace),
            remittance.total,
            paid,
        ));
    }
    Ok(())
}

/// Reads and checks a whole interchange. Nothing is returned unless all of it is good.
pub fn parse(text: &str) -> Result<Interchange, String> {
    let (segments, component) = segments(text)?;
    let mut iter = segments.iter();
    let isa = iter.next().ok_or("the file is empty")?;
    let isa_control = isa.get(13);
    let marked_as_test = isa.get(15) == "T";

    let mut remittances = Vec::new();
    let mut groups = 0usize;
    let mut closed = false;
    let mut group_control: Option<&str> = None;
    let mut in_group = 0usize;
    let mut open: Option<(&str, usize, Builder)> = None;

    for segment in iter {
        if closed {
            return Err("there is more after the IEA segment, which phv does not read".into());
        }
        if let Some((control, count, builder)) = open.as_mut() {
            *count += 1;
            if segment.id != "SE" {
                builder.read(segment, component)?;
                continue;
            }
            if segment.required(1)? != count.to_string() {
                return Err(format!(
                    "transaction {control}: SE says {} segments, but there are {count}",
                    segment.get(1)
                ));
            }
            if segment.get(2) != *control {
                return Err(format!("SE02 does not match transaction {control}"));
            }
            let (_, _, builder) = open.take().expect("checked above");
            remittances.push(builder.finish()?);
            in_group += 1;
            continue;
        }
        match segment.id {
            "GS" => {
                if segment.get(1) != "HP" || !segment.get(8).starts_with("005010X221") {
                    return Err(format!(
                        "this is not a 5010 835: GS01 is {:?} and GS08 is {:?}",
                        segment.get(1),
                        segment.get(8)
                    ));
                }
                group_control = Some(segment.get(6));
                in_group = 0;
                groups += 1;
            }
            "ST" => {
                if segment.get(1) != "835" {
                    return Err(format!(
                        "this is an X12 {} file, and phv only reads 835 remittances",
                        segment.get(1)
                    ));
                }
                if group_control.is_none() {
                    return Err("an ST segment comes before any GS segment".into());
                }
                open = Some((segment.required(2)?, 1, Builder::default()));
            }
            "GE" => {
                if segment.get(1) != in_group.to_string() {
                    return Err(format!(
                        "GE says {} transactions, but there are {in_group}",
                        segment.get(1)
                    ));
                }
                if Some(segment.get(2)) != group_control {
                    return Err("GE02 does not match its GS segment".into());
                }
                group_control = None;
            }
            "IEA" => {
                if segment.get(1) != groups.to_string() {
                    return Err(format!(
                        "IEA says {} groups, but there are {groups}",
                        segment.get(1)
                    ));
                }
                if segment.get(2) != isa_control {
                    return Err("IEA02 does not match the ISA segment".into());
                }
                closed = true;
            }
            other => return Err(format!("unexpected {other} segment outside a transaction")),
        }
    }
    if open.is_some() || group_control.is_some() || !closed {
        return Err("this file stops early: a closing SE, GE or IEA segment is missing".into());
    }
    if remittances.is_empty() {
        return Err("this file has no remittances".into());
    }
    Ok(Interchange {
        marked_as_test,
        remittances,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_exact() {
        assert_eq!(Cents::parse("400.72"), Ok(Cents(40072)));
        assert_eq!(Cents::parse("7"), Ok(Cents(700)));
        assert_eq!(Cents::parse("7.5"), Ok(Cents(750)));
        assert_eq!(Cents::parse(".50"), Ok(Cents(50)));
        assert_eq!(Cents::parse("-12.05"), Ok(Cents(-1205)));
        assert_eq!(Cents(-1205).text(), "-12.05");
        assert_eq!(Cents(5).text(), "0.05");
        for bad in ["", ".", "1.234", "1,000.00", "abc", "1.2.3", "--1"] {
            assert!(Cents::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn postal_codes_get_their_hyphen() {
        assert_eq!(postal_code("627045106"), "62704-5106");
        assert_eq!(postal_code("60601"), "60601");
    }
}
