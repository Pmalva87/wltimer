//! Competition documents: a meet, its attempts, and what they add up to.
//!
//! A competition is not a workout. There are no phases to flatten and no clock
//! the app owns, so it is a document kind of its own rather than a `Workout`
//! with the timing left blank. What it shares with everything else here is the
//! format — markdown carrying `- id:` and `- updated:` under the title — which
//! is what lets a meet leave the phone and come back through the same export,
//! re-import and restore paths as a workout or a plan.
//!
//! The two lifts are fixed rather than a list of named sections. A snatch and
//! a clean & jerk are not two rows that happen to have names: the total is
//! *defined* as the sum of their bests, the warmup room is organised around
//! them, and making the set configurable would leave every screen asking which
//! lifts this meet has in exchange for a generality the sport does not have.
//!
//! Nothing in here reads the clock or the disk, and the arithmetic that
//! matters at a meet — what you are on, what you still need, whether a
//! qualifying total is still live — is plain functions over the document.

use crate::ids;
use crate::parser::ParseError;
use serde::{Deserialize, Serialize};

/// Attempts per lift. Three, always: a slot with nothing written in it is an
/// attempt you have not declared yet, not an attempt you do not have.
pub const ATTEMPTS: usize = 3;

/// Smallest legal increment between attempts, in kg.
const MIN_INCREMENT: f64 = 1.0;

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

// ---- model ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lift {
    Snatch,
    CleanJerk,
}

impl Lift {
    pub const ALL: [Lift; 2] = [Lift::Snatch, Lift::CleanJerk];

    /// The heading this lift is written as.
    pub fn heading(self) -> &'static str {
        match self {
            Lift::Snatch => "Snatch",
            Lift::CleanJerk => "Clean & Jerk",
        }
    }

    /// Read a `## Heading` as a lift. Deliberately forgiving about how the
    /// clean & jerk is spelled — every federation, coach and notebook writes
    /// it differently, and none of them is wrong.
    pub fn from_heading(h: &str) -> Option<Lift> {
        let norm: String = h
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        match norm.as_str() {
            "snatch" | "sn" => Some(Lift::Snatch),
            "cleanjerk" | "cleanandjerk" | "cj" | "clean" | "jerk" => Some(Lift::CleanJerk),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptResult {
    /// Declared, not yet taken. The weight is a plan and can still change.
    Declared,
    Good,
    Miss,
}

impl AttemptResult {
    /// Taken attempts are spent, good or bad. The distinction the screens
    /// need far more often than good/miss is "can this one still change".
    pub fn taken(self) -> bool {
        self != AttemptResult::Declared
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub kg: f64,
    pub result: AttemptResult,
}

/// One warmup set: a weight, how many reps, and whether you have hit it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WarmupSet {
    pub kg: f64,
    pub reps: u32,
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiftEntry {
    /// Fixed three slots; `None` is an attempt not declared yet.
    pub attempts: [Option<Attempt>; ATTEMPTS],
    pub warmup: Vec<WarmupSet>,
    /// Free markdown from the section, shown under the lift.
    pub notes_md: String,
}

/// A total worth chasing: a qualifying mark, a selection standard, a number
/// you want today. Just the two things every one of them has — how much, and
/// what it is called. What makes a mark *count* is [`Qualification`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub total: f64,
    pub label: String,
}

/// What it takes to get into a competition.
///
/// This hangs off the meet you are trying to **enter**, not the meet you are
/// lifting at, because that is where the rule actually lives: Europeans says
/// what Europeans wants. A mark is only ever a mark *for* something, and it
/// is bounded two ways — a qualifying window, and which meets are allowed to
/// produce the result. A 250 total at the right meet last March and a 250 at a
/// club open next week are not the same thing, and an app that stores a bare
/// number cannot tell you which one you have.
///
/// So the document for a meet you have not been to yet is a perfectly good
/// document: a name, a date, and what it would take. If you get in, the same
/// document grows attempts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Qualification {
    /// Results before this date do not count. `None` — no lower bound.
    #[serde(default)]
    pub from: Option<String>,
    /// Results after this date do not count. `None` — no upper bound.
    #[serde(default)]
    pub to: Option<String>,
    /// Sanctioning bodies whose meets count, as written. Empty means any meet
    /// counts, which is also what a mark you set yourself means.
    #[serde(default)]
    pub counts: Vec<String>,
    /// The marks themselves. A table rather than a number, because that is
    /// what a qualifying standard is once masters are involved: a row per age
    /// group and bodyweight category, and only one of them is yours.
    #[serde(default)]
    pub standards: Vec<Standard>,
}

/// One row of a qualifying table: how much, for whom.
///
/// An absent qualifier means the row does not care — a meet with a single
/// entry standard is a one-row table with neither set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Standard {
    pub total: f64,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub age_group: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
}

impl Standard {
    /// Does this row apply to a lifter entering in this group?
    ///
    /// A qualifier the row states and the entry does not is *not* a match.
    /// Falling back to another group's number would be worse than saying
    /// nothing: it is a number you would train towards and then not be held
    /// to, or be held to and miss.
    pub fn applies_to(&self, age_group: Option<&str>, category: Option<&str>) -> bool {
        let ok = |row: Option<&String>, entry: Option<&str>, eq: fn(&str, &str) -> bool| match row {
            None => true,
            Some(r) => entry.is_some_and(|e| eq(r, e)),
        };
        ok(self.age_group.as_ref(), age_group, age_groups_match)
            && ok(self.category.as_ref(), category, categories_match)
    }

    /// How narrowly this row is aimed — the tie-break when more than one
    /// applies, so `M40 89 kg` beats a bare mark rather than racing it.
    fn specificity(&self) -> u8 {
        u8::from(self.age_group.is_some()) + u8::from(self.category.is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Competition {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    /// Local date, `YYYY-MM-DD`. Optional: a meet you have entered but whose
    /// date you do not know yet is still worth writing down.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub bodyweight: Option<f64>,
    /// The weight class, verbatim. Not an enum and not validated: the IWF has
    /// rewritten the category list twice in recent memory, and an app that
    /// owns that list strands every document the next time it changes.
    #[serde(default)]
    pub category: Option<String>,
    /// Who sanctioned the meet, as written — a federation, a series, a club.
    /// Free text for the same reason `category` is: this is the field another
    /// meet's [`Qualification`] tests, and no list the app ships would keep up
    /// with the ones a lifter actually competes under.
    ///
    /// A list, because one meet often answers to more than one body, and that
    /// is exactly what decides whether it travels: an international held in
    /// Portugal can be the thing that qualifies you for something in England.
    #[serde(default)]
    pub orgs: Vec<String>,
    /// The age group entered — `M40`, `40-44`, `Senior`. On a meet you have
    /// lifted at, the group you were in; on one you are chasing, the group you
    /// intend to enter, which is what picks your row out of its table.
    #[serde(default)]
    pub age_group: Option<String>,
    /// Totals to chase *today*: a goal, a rival's number, a round figure.
    /// Unbounded by design — a qualifying mark is a [`Qualification`] on the
    /// meet it qualifies you for, not a target on this one.
    #[serde(default)]
    pub targets: Vec<Target>,
    /// What it takes to get into this meet, if it is one you are chasing.
    #[serde(default)]
    pub qualification: Option<Qualification>,
    pub snatch: LiftEntry,
    pub clean_jerk: LiftEntry,
}

impl Competition {
    pub fn new(name: &str) -> Competition {
        Competition {
            id: None,
            name: name.to_string(),
            date: None,
            bodyweight: None,
            category: None,
            orgs: Vec::new(),
            age_group: None,
            targets: Vec::new(),
            qualification: None,
            snatch: LiftEntry::default(),
            clean_jerk: LiftEntry::default(),
        }
    }

    pub fn lift(&self, lift: Lift) -> &LiftEntry {
        match lift {
            Lift::Snatch => &self.snatch,
            Lift::CleanJerk => &self.clean_jerk,
        }
    }

    pub fn lift_mut(&mut self, lift: Lift) -> &mut LiftEntry {
        match lift {
            Lift::Snatch => &mut self.snatch,
            Lift::CleanJerk => &mut self.clean_jerk,
        }
    }
}

// ---- what it adds up to ----

/// What the competition totals right now.
///
/// Three states rather than an `Option<f64>`, because "no total yet" and "no
/// total, ever" are the difference between a meet in progress and a meet that
/// is over — and a bombed-out lift must never read as a zero that quietly
/// sums into a total.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TotalState {
    /// Both lifts have a good attempt.
    Made { total: f64 },
    /// Not both yet, but there are attempts left to get there.
    Open,
    /// A lift is out of attempts with nothing made. There is no total.
    BombedOut,
}

impl TotalState {
    pub fn kg(self) -> Option<f64> {
        match self {
            TotalState::Made { total } => Some(total),
            _ => None,
        }
    }
}

/// Where a target stands, from the platform's point of view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TargetStatus {
    /// Already made — the bar can do nothing to take it away.
    Clinched,
    /// One lift is banked and the other is live, so the target reduces to a
    /// single number on a single attempt.
    Needs { lift: Lift, attempt: u8, kg: f64 },
    /// Still reachable, but not yet down to one attempt — both lifts are
    /// unfinished, so what you need here depends on what you do there.
    Open,
    /// Unreachable: the meet is over below it, or a lift is bombed.
    OutOfReach,
}

impl LiftEntry {
    /// Heaviest good lift, which is the only attempt that counts for anything.
    pub fn best(&self) -> Option<f64> {
        self.attempts
            .iter()
            .flatten()
            .filter(|a| a.result == AttemptResult::Good)
            .map(|a| a.kg)
            .fold(None, |acc: Option<f64>, kg| Some(acc.map_or(kg, |b| b.max(kg))))
    }

    /// The best this lift could still come to if every attempt already
    /// declared is made. A slot with nothing written in it cannot be counted,
    /// so this only ever goes up as you declare — it is a projection of your
    /// own plan, not a limit on what you may do.
    pub fn best_possible(&self) -> Option<f64> {
        self.attempts
            .iter()
            .flatten()
            .filter(|a| a.result != AttemptResult::Miss)
            .map(|a| a.kg)
            .fold(self.best(), |acc: Option<f64>, kg| {
                Some(acc.map_or(kg, |b| b.max(kg)))
            })
    }

    pub fn taken(&self) -> usize {
        self.attempts
            .iter()
            .flatten()
            .filter(|a| a.result.taken())
            .count()
    }

    pub fn attempts_left(&self) -> usize {
        ATTEMPTS - self.taken()
    }

    /// Three attempts taken, none of them good.
    pub fn bombed_out(&self) -> bool {
        self.best().is_none() && self.attempts_left() == 0
    }

    /// The next attempt still to take, 1-based.
    pub fn next_attempt(&self) -> Option<u8> {
        self.attempts
            .iter()
            .position(|a| a.is_none_or(|a| !a.result.taken()))
            .map(|i| i as u8 + 1)
    }

    /// Lightest weight the next attempt may legally be: the bar goes up after
    /// a good lift and may be repeated after a miss, and it never goes down.
    pub fn min_next_kg(&self) -> Option<f64> {
        let last = self
            .attempts
            .iter()
            .flatten()
            .rfind(|a| a.result.taken())?;
        Some(match last.result {
            AttemptResult::Good => last.kg + MIN_INCREMENT,
            _ => last.kg,
        })
    }

    /// Attempts declared lighter than the one before them, 1-based.
    ///
    /// A warning, never an error, in the style of
    /// [`crate::model::Workout::parts_without_rest_after`]: the document is
    /// free to say anything, but a bar that goes down is a slip every time.
    pub fn attempts_going_down(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut prev: Option<f64> = None;
        for (i, slot) in self.attempts.iter().enumerate() {
            let Some(a) = slot else { continue };
            if prev.is_some_and(|p| a.kg < p) {
                out.push(i as u8 + 1);
            }
            prev = Some(a.kg);
        }
        out
    }
}

impl Competition {
    pub fn total(&self) -> TotalState {
        if self.snatch.bombed_out() || self.clean_jerk.bombed_out() {
            return TotalState::BombedOut;
        }
        match (self.snatch.best(), self.clean_jerk.best()) {
            (Some(s), Some(c)) => TotalState::Made { total: s + c },
            _ => TotalState::Open,
        }
    }

    /// The total if every attempt already declared is made.
    pub fn best_possible_total(&self) -> Option<f64> {
        match (self.snatch.best_possible(), self.clean_jerk.best_possible()) {
            (Some(s), Some(c)) => Some(s + c),
            _ => None,
        }
    }

    /// Whether a total can still happen at all.
    fn finished(&self) -> bool {
        self.snatch.attempts_left() == 0 && self.clean_jerk.attempts_left() == 0
    }

    /// Is this lift over — is what it contributes to the total now fixed?
    ///
    /// Out of attempts, or overtaken by the order of the meet: the snatch is
    /// done before the clean & jerk begins, so a taken clean & jerk closes it
    /// whatever its slots say. That second rule is not pedantry — a lifter who
    /// writes down the 95 they made and not the two they missed would
    /// otherwise leave the snatch reading as live for the rest of the day, and
    /// every clean & jerk number on the screen with it.
    fn closed(&self, lift: Lift) -> bool {
        match lift {
            Lift::Snatch => self.snatch.attempts_left() == 0 || self.clean_jerk.taken() > 0,
            Lift::CleanJerk => self.clean_jerk.attempts_left() == 0,
        }
    }

    /// What this target needs, right now.
    ///
    /// The useful answer is a single weight on a single attempt, and that is
    /// only honest once one lift is banked: during the snatch, what you need
    /// on the bar depends on a clean & jerk you have not taken yet, so the
    /// status stays `Open` and the screen shows the target total instead of
    /// inventing a number.
    ///
    /// `OutOfReach` is only ever *proved*, never guessed — a remaining attempt
    /// may be declared at any weight, so nothing is out of reach while one is
    /// still in hand.
    pub fn target_status(&self, total: f64) -> TargetStatus {
        if self.total().kg().is_some_and(|t| t >= total) {
            return TargetStatus::Clinched;
        }
        if self.snatch.bombed_out() || self.clean_jerk.bombed_out() || self.finished() {
            return TargetStatus::OutOfReach;
        }
        // One lift still live, the other finished and on the board. Both
        // halves matter: a banked snatch you can still add to is not a number
        // to subtract from, it is a number that may yet improve.
        let live = Lift::ALL.into_iter().find(|&l| {
            let other = match l {
                Lift::Snatch => Lift::CleanJerk,
                Lift::CleanJerk => Lift::Snatch,
            };
            !self.closed(l)
                && self.lift(l).attempts_left() > 0
                && self.closed(other)
                && self.lift(other).best().is_some()
        });
        let Some(lift) = live else {
            return TargetStatus::Open;
        };
        let banked = match lift {
            Lift::Snatch => self.clean_jerk.best(),
            Lift::CleanJerk => self.snatch.best(),
        };
        let entry = self.lift(lift);
        let (Some(banked), Some(attempt)) = (banked, entry.next_attempt()) else {
            return TargetStatus::Open;
        };
        let needed = total - banked;
        let kg = entry.min_next_kg().map_or(needed, |min| needed.max(min));
        TargetStatus::Needs { lift, attempt, kg }
    }
}

// ---- what counts toward what ----

/// Letters and digits only, lowercased — the shape every comparison here
/// starts from, since the same group is written six ways by six federations.
fn squash(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// An age group reduced to what it actually says: a sex letter, if it carries
/// one, and the age the band starts at. `M40`, `Masters 40`, `40-44` and
/// `40+` all come back the same, which is the point — the app must not make a
/// lifter write the group the way it happens to spell it.
fn age_key(s: &str) -> (Option<char>, Option<u32>) {
    let lower = s.to_lowercase();
    // The *first* run of digits, read off the original rather than a squashed
    // copy: `40-44` names the band it starts at, and squashing it first would
    // read the pair as one number.
    let digits: String = lower
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let sex = lower
        .chars()
        .find(char::is_ascii_alphabetic)
        .filter(|c| matches!(c, 'm' | 'w' | 'f'));
    (sex, digits.parse().ok())
}

/// Are these the same age group?
///
/// Two banded groups match on the band; a sex letter only has to agree when
/// both sides state one, so a table written `40-44` still finds an `M40`
/// entry. Groups with no number in them — senior, junior — are compared as
/// words, since there is nothing else to go on.
fn age_groups_match(a: &str, b: &str) -> bool {
    match (age_key(a), age_key(b)) {
        ((sa, Some(x)), (sb, Some(y))) => x == y && (sa.is_none() || sb.is_none() || sa == sb),
        ((_, None), (_, None)) => squash(a) == squash(b),
        _ => false,
    }
}

/// A bodyweight category as the pair that distinguishes it: whether it is the
/// open class above a weight, and the weight itself. `89`, `89 kg`, `-89` and
/// `89kg` are one class; `+89` is a different one, and must never match it.
fn category_key(s: &str) -> Option<(bool, u32)> {
    let t = s.trim();
    let plus = t.starts_with('+');
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok().map(|kg| (plus, kg))
}

fn categories_match(a: &str, b: &str) -> bool {
    match (category_key(a), category_key(b)) {
        (Some(x), Some(y)) => x == y,
        _ => squash(a) == squash(b),
    }
}

impl Qualification {
    /// Does a result at this meet count toward the mark at all?
    ///
    /// Two independent gates, both of which have to say yes: the meet's date
    /// falls inside the window, and its sanctioning body is one of the ones
    /// named. Dates compare as strings — `YYYY-MM-DD` sorts chronologically,
    /// the same property the timestamps are written for.
    ///
    /// An undated meet fails a window rather than passing it. The result may
    /// well have been inside it, but nothing in the document says so, and a
    /// qualification you cannot evidence is one you do not have.
    pub fn accepts(&self, meet: &Competition) -> bool {
        if self.from.is_some() || self.to.is_some() {
            let Some(date) = meet.date.as_deref() else {
                return false;
            };
            if self.from.as_deref().is_some_and(|f| date < f) {
                return false;
            }
            if self.to.as_deref().is_some_and(|t| date > t) {
                return false;
            }
        }
        if self.counts.is_empty() {
            return true;
        }
        // Both sides are lists, so this is an overlap rather than an equality:
        // a meet sanctioned by two bodies counts wherever either one does.
        self.counts
            .iter()
            .any(|c| meet.orgs.iter().any(|o| squash(o) == squash(c)))
    }

    /// The rows of this table that apply to a lifter entering in this group,
    /// most specific first.
    ///
    /// An entry that states neither group gets the whole table back rather
    /// than nothing: you have not said which row is yours, and showing all of
    /// them is the honest answer to that.
    pub fn applicable(&self, age_group: Option<&str>, category: Option<&str>) -> Vec<&Standard> {
        let mut rows: Vec<&Standard> = if age_group.is_none() && category.is_none() {
            self.standards.iter().collect()
        } else {
            self.standards
                .iter()
                .filter(|s| s.applies_to(age_group, category))
                .collect()
        };
        rows.sort_by_key(|s| std::cmp::Reverse(s.specificity()));
        rows
    }

    /// The one row that applies, when exactly one should.
    pub fn standard_for(&self, age_group: Option<&str>, category: Option<&str>) -> Option<&Standard> {
        self.applicable(age_group, category).first().copied()
    }

    /// The meet that already satisfies `standard`, if one does — the first in
    /// `results` that both counts and totals enough.
    ///
    /// The row's own age group and category are deliberately *not* required of
    /// the result. A qualifying window is long enough to age up inside, and
    /// which class a past total was set in is a question federations answer
    /// differently. The meet that comes back carries its own group, so the
    /// screen can show where the total was set and let its lifter judge.
    pub fn met_by<'a>(&self, standard: &Standard, results: &'a [Competition]) -> Option<&'a Competition> {
        results
            .iter()
            .filter(|m| self.accepts(m))
            .find(|m| m.total().kg().is_some_and(|t| t >= standard.total))
    }
}

/// Marks a result at `meet` could still satisfy, gathered from the meets
/// `chasing` holds the qualifications for.
///
/// This is what turns a competition into a platform screen worth reading: not
/// "your targets", but every door today's meet is actually able to open, with
/// the ones it cannot left out. A mark already satisfied elsewhere is dropped
/// too — it is no longer something to lift for.
pub fn marks_in_play<'a>(
    meet: &Competition,
    chasing: &'a [Competition],
    results: &'a [Competition],
) -> Vec<(&'a Competition, &'a Standard)> {
    let mut out = Vec::new();
    for c in chasing {
        let Some(q) = &c.qualification else { continue };
        if !q.accepts(meet) {
            continue;
        }
        // Which row is yours is a property of the entry you intend *there*,
        // which is what that meet's own group and category say — so no profile
        // has to be consulted, and a meet you would enter in two different
        // classes is two documents, as it should be.
        for standard in q.applicable(c.age_group.as_deref(), c.category.as_deref()) {
            if q.met_by(standard, results).is_none() {
                out.push((c, standard));
            }
        }
    }
    out
}

// ---- Sinclair ----

/// The two numbers behind the Sinclair coefficient, which the IWF reissues
/// every Olympiad. They are data rather than constants for exactly that
/// reason: when the cycle turns, a stored document is edited, not the app.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SinclairCoefficients {
    /// The `A` of the official table.
    pub a: f64,
    /// `b`: bodyweight of the world record holder in the heaviest class.
    pub b: f64,
}

/// Sinclair points for a total at a bodyweight — a total scaled as if lifted
/// by an athlete in the heaviest class, so two totals at different bodyweights
/// can be compared.
///
/// `10^(A·log10(x/b)²) · total` for a bodyweight `x` under `b`, and the total
/// itself at or above it.
pub fn sinclair(total: f64, bodyweight: f64, c: SinclairCoefficients) -> Option<f64> {
    if !(total.is_finite() && bodyweight.is_finite()) || total <= 0.0 || bodyweight <= 0.0 {
        return None;
    }
    if bodyweight >= c.b {
        return Some(total);
    }
    let x = (bodyweight / c.b).log10();
    Some(10f64.powf(c.a * x * x) * total)
}

// ---- markdown ----

/// Format a weight the way it is written: `95`, `42.5`.
pub fn fmt_kg(kg: f64) -> String {
    if (kg - kg.round()).abs() < 1e-9 {
        format!("{}", kg.round() as i64)
    } else {
        let s = format!("{kg:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn parse_kg(val: &str) -> Option<f64> {
    let kg: f64 = val.trim().parse().ok()?;
    (kg.is_finite() && kg > 0.0 && kg < 1000.0).then_some(kg)
}

fn parse_result(word: &str) -> Option<AttemptResult> {
    let norm: String = word
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    match norm.as_str() {
        "good" | "made" | "o" | "y" | "yes" => Some(AttemptResult::Good),
        "miss" | "missed" | "nolift" | "no" | "x" | "fail" | "failed" => Some(AttemptResult::Miss),
        _ => None,
    }
}

/// A `- [x] 60 x 2` warmup line: weight, optional reps, ticked or not.
///
/// Recognised anywhere in a lift's section, not only under a `### Warmup`
/// heading, so the heading is a courtesy to whoever reads the file rather
/// than something the format depends on.
fn warmup_line(line: &str) -> Option<WarmupSet> {
    let t = line.trim();
    let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))?;
    let ticked = rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]"));
    let (mark, rest) = match ticked {
        Some(r) => (true, r),
        None => (false, rest.strip_prefix("[ ]").or_else(|| rest.strip_prefix("[]"))?),
    };
    let body = rest.trim();
    let (kg, reps) = match body.split_once(['x', 'X', '*', '@']) {
        Some((kg, reps)) => (parse_kg(kg)?, reps.trim().parse::<u32>().ok()?),
        None => (parse_kg(body)?, 1),
    };
    Some(WarmupSet {
        kg,
        reps,
        done: mark,
    })
}

fn is_warmup_heading(h: &str) -> bool {
    let norm: String = h
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    norm == "warmup"
}

/// Does this document claim to be a competition?
///
/// The `- kind:` bullet is what lets an upload be routed by what the file is
/// rather than by which button it arrived through — the rule a backup bundle
/// already follows. A competition and a workout are both `# Title` + `##`
/// documents, and nothing else about them is reliably different.
pub fn looks_like_competition(src: &str) -> bool {
    for line in src.lines() {
        if line.trim_start().starts_with("## ") {
            break;
        }
        if let Some((k, v)) = ids::bullet(line) {
            if k == "kind" && v.trim().eq_ignore_ascii_case("competition") {
                return true;
            }
        }
    }
    false
}

/// The heading a qualification is written under.
const QUALIFICATION: &str = "Qualification";

/// Words that name an age group on their own.
const AGE_WORDS: [&str; 6] = ["senior", "junior", "youth", "master", "masters", "open"];

/// Read a `### M40 89 kg A group` row heading as the group it names.
///
/// A token is a bodyweight category when it says so — a `kg`, a `+` or a `-`.
/// Anything else carrying a digit is an age group, which is the reading that
/// keeps `M40`, `40-44` and `Masters 40` all working; the rest is the row's
/// own name. Ambiguity resolves towards the age group because that is the
/// half a masters table always states and a category is usually spelled out.
fn parse_row_heading(h: &str) -> (Option<String>, Option<String>, String) {
    let tokens: Vec<&str> = h.split_whitespace().collect();
    let (mut age, mut category, mut label) = (Vec::new(), Vec::new(), Vec::new());
    let mut skip = false;
    for (i, tok) in tokens.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        let squashed = squash(tok);
        let has_digit = squashed.chars().any(|c| c.is_ascii_digit());
        let next_is_kg = tokens.get(i + 1).is_some_and(|n| squash(n) == "kg");
        let says_kg = squashed.ends_with("kg") || tok.starts_with('+') || tok.starts_with('-');
        if has_digit && (says_kg || next_is_kg) {
            category.push(tok.trim_end_matches("kg").to_string());
            skip = next_is_kg;
        } else if has_digit || AGE_WORDS.contains(&squashed.as_str()) {
            age.push(tok.to_string());
        } else {
            label.push(*tok);
        }
    }
    let join = |v: Vec<String>| (!v.is_empty()).then(|| v.join(" "));
    (join(age), join(category), label.join(" "))
}

fn is_qualification_heading(h: &str) -> bool {
    let norm = h.to_lowercase();
    norm.starts_with("qualif") || norm.starts_with("standard")
}

/// `250 A group`, or just `250` when the mark has no name.
fn mark_line(total: f64, label: &str) -> String {
    format!("{} {}", fmt_kg(total), label).trim_end().to_string()
}

/// Serialize a competition to the markdown that [`parse_competition`] reads.
/// Parsing the result yields an equal competition.
pub fn competition_to_markdown(c: &Competition) -> String {
    let mut out = format!("# {}\n", c.name);
    if let Some(id) = &c.id {
        out.push_str(&format!("- id: {id}\n"));
    }
    out.push_str("- kind: competition\n");
    if let Some(d) = &c.date {
        out.push_str(&format!("- date: {d}\n"));
    }
    if let Some(bw) = c.bodyweight {
        out.push_str(&format!("- bodyweight: {}\n", fmt_kg(bw)));
    }
    if let Some(cat) = &c.category {
        out.push_str(&format!("- category: {cat}\n"));
    }
    if !c.orgs.is_empty() {
        out.push_str(&format!("- org: {}\n", c.orgs.join(", ")));
    }
    if let Some(g) = &c.age_group {
        out.push_str(&format!("- age group: {g}\n"));
    }
    for t in &c.targets {
        out.push_str(&format!("- target: {}\n", mark_line(t.total, &t.label)));
    }
    if let Some(q) = &c.qualification {
        out.push_str(&format!("\n## {QUALIFICATION}\n"));
        if let Some(from) = &q.from {
            out.push_str(&format!("- from: {from}\n"));
        }
        if let Some(to) = &q.to {
            out.push_str(&format!("- to: {to}\n"));
        }
        if !q.counts.is_empty() {
            out.push_str(&format!("- counts: {}\n", q.counts.join(", ")));
        }
        // A single mark stays a single line; a table becomes one `###` row per
        // group, which is both how a federation publishes it and the shortest
        // thing to write by hand.
        for row in q.standards.iter().filter(|s| s.specificity() == 0) {
            out.push_str(&format!("- needs: {}\n", mark_line(row.total, &row.label)));
        }
        for row in q.standards.iter().filter(|s| s.specificity() > 0) {
            // The `kg` is what tells a reader — and the parser — that the
            // number is a bodyweight class and not part of the age group.
            let category = row.category.as_deref().map(|c| {
                if squash(c).ends_with("kg") {
                    c.to_string()
                } else {
                    format!("{c} kg")
                }
            });
            let heading = [
                row.age_group.as_deref().unwrap_or(""),
                category.as_deref().unwrap_or(""),
                &row.label,
            ]
            .iter()
            .filter(|p| !p.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
            out.push_str(&format!("\n### {heading}\n- needs: {}\n", fmt_kg(row.total)));
        }
    }
    for lift in Lift::ALL {
        let e = c.lift(lift);
        out.push_str(&format!("\n## {}\n", lift.heading()));
        for (i, slot) in e.attempts.iter().enumerate() {
            let Some(a) = slot else { continue };
            let result = match a.result {
                AttemptResult::Declared => String::new(),
                AttemptResult::Good => " good".into(),
                AttemptResult::Miss => " miss".into(),
            };
            out.push_str(&format!("- {}: {}{}\n", i + 1, fmt_kg(a.kg), result));
        }
        if !e.warmup.is_empty() {
            out.push_str("\n### Warmup\n");
            for w in &e.warmup {
                let mark = if w.done { "x" } else { " " };
                out.push_str(&format!("- [{}] {} x {}\n", mark, fmt_kg(w.kg), w.reps));
            }
        }
        if !e.notes_md.is_empty() {
            out.push('\n');
            out.push_str(&e.notes_md);
            out.push('\n');
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Preamble,
    Lift(Lift),
    Qualification,
    /// Under a heading the format does not know.
    Dropped,
}

/// Parse a markdown competition document.
///
/// `# Title` names the meet, preamble bullets carry the date, bodyweight,
/// category and targets, and each `## Snatch` / `## Clean & Jerk` section
/// holds up to three `- <n>: <kg> [good|miss]` attempts plus a warmup
/// checklist. Everything else in a section is free markdown kept as notes.
pub fn parse_competition(src: &str) -> Result<Competition, Vec<ParseError>> {
    let mut errors: Vec<ParseError> = Vec::new();
    let mut name: Option<String> = None;
    let mut c = Competition::new("");
    let mut current = Section::Preamble;
    let mut seen: Vec<Lift> = Vec::new();
    let mut qual: Option<QualBuilder> = None;

    for (i, raw) in src.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim();

        if let Some(h) = trimmed.strip_prefix("## ") {
            let h = h.trim();
            current = if is_qualification_heading(h) {
                qual.get_or_insert_with(QualBuilder::default);
                Section::Qualification
            } else {
                match Lift::from_heading(h) {
                    Some(lift) if seen.contains(&lift) => {
                        errors.push(err(line_no, format!("duplicate '{}' section", lift.heading())));
                        Section::Lift(lift)
                    }
                    Some(lift) => {
                        seen.push(lift);
                        Section::Lift(lift)
                    }
                    None => {
                        errors.push(err(
                            line_no,
                            format!("unknown section '{h}' — a competition has '## Snatch', '## Clean & Jerk' and '## {QUALIFICATION}'"),
                        ));
                        // Nothing under an unknown heading is filed anywhere:
                        // guessing which lift it meant would put attempts on
                        // the wrong bar.
                        Section::Dropped
                    }
                }
            };
            continue;
        }
        if let Some(h) = trimmed.strip_prefix("# ") {
            if name.is_none() && current == Section::Preamble {
                name = Some(h.trim().to_string());
            } else {
                errors.push(err(line_no, "unexpected extra '#' title"));
            }
            continue;
        }

        let lift = match current {
            Section::Preamble => {
                if let Some((key, val)) = ids::bullet(trimmed) {
                    preamble(&mut c, &key, val, line_no, &mut errors);
                }
                continue;
            }
            Section::Qualification => {
                let b = qual.get_or_insert_with(QualBuilder::default);
                if let Some(h) = trimmed.strip_prefix("### ") {
                    b.heading(h.trim(), line_no);
                } else if let Some((key, val)) = ids::bullet(trimmed) {
                    qualification(b, &key, val, line_no, &mut errors);
                }
                continue;
            }
            Section::Dropped => continue,
            Section::Lift(lift) => lift,
        };

        if trimmed.strip_prefix("### ").is_some_and(is_warmup_heading) {
            continue;
        }
        if let Some(set) = warmup_line(trimmed) {
            c.lift_mut(lift).warmup.push(set);
            continue;
        }
        if let Some((key, val)) = ids::bullet(trimmed) {
            if let Some(n) = attempt_number(&key) {
                attempt(c.lift_mut(lift), n, val, line_no, &mut errors);
                continue;
            }
        }
        let notes = &mut c.lift_mut(lift).notes_md;
        notes.push_str(raw);
        notes.push('\n');
    }

    for lift in Lift::ALL {
        let notes = &mut c.lift_mut(lift).notes_md;
        *notes = notes.trim().to_string();
    }
    c.qualification = qual.map(|b| b.finish(&mut errors));
    match name {
        Some(n) => c.name = n,
        None => errors.push(err(
            1,
            "missing competition title — start the document with '# My Meet'",
        )),
    }

    if errors.is_empty() {
        Ok(c)
    } else {
        errors.sort_by_key(|e| e.line);
        Err(errors)
    }
}

/// `1` / `attempt 1` / `snatch 1` all name the same slot.
fn attempt_number(key: &str) -> Option<usize> {
    let digits = key.rsplit(' ').next()?;
    let n: usize = digits.parse().ok()?;
    Some(n)
}

fn attempt(entry: &mut LiftEntry, n: usize, val: &str, line: usize, errors: &mut Vec<ParseError>) {
    if !(1..=ATTEMPTS).contains(&n) {
        errors.push(err(line, format!("attempt '{n}' — a lift has attempts 1 to {ATTEMPTS}")));
        return;
    }
    if entry.attempts[n - 1].is_some() {
        errors.push(err(line, format!("duplicate attempt {n}")));
        return;
    }
    let (kg_str, rest) = val.trim().split_once(' ').unwrap_or((val.trim(), ""));
    let Some(kg) = parse_kg(kg_str) else {
        errors.push(err(line, format!("invalid weight '{kg_str}' in attempt {n}")));
        return;
    };
    let result = if rest.trim().is_empty() {
        AttemptResult::Declared
    } else {
        match parse_result(rest) {
            Some(r) => r,
            None => {
                errors.push(err(
                    line,
                    format!("unknown result '{}' — write 'good', 'miss', or nothing at all", rest.trim()),
                ));
                return;
            }
        }
    };
    entry.attempts[n - 1] = Some(Attempt { kg, result });
}

/// A `- <kg> <label>` mark, as both `- target:` and `- needs:` are written.
fn parse_mark(val: &str) -> Option<Target> {
    let (kg, label) = val.trim().split_once(' ').unwrap_or((val.trim(), ""));
    Some(Target {
        total: parse_kg(kg)?,
        label: label.trim().to_string(),
    })
}

/// A table row under construction: the total arrives on its own line, and a
/// row that never gets one is an error rather than a silent zero.
#[derive(Default)]
struct RowBuilder {
    line: usize,
    total: Option<f64>,
    label: String,
    age_group: Option<String>,
    category: Option<String>,
    /// From a `###` heading rather than the section's own bullets, which is
    /// what makes a second `- needs:` under it a mistake instead of a row.
    headed: bool,
}

/// Everything a `## Qualification` section builds while it is being read.
#[derive(Default)]
struct QualBuilder {
    q: Qualification,
    rows: Vec<RowBuilder>,
    /// Group bullets written at section level, qualifying every row that does
    /// not say otherwise — the natural way to write a table that is all one
    /// category with a mark per entry group.
    age_group: Option<String>,
    category: Option<String>,
    current: Option<usize>,
}

impl QualBuilder {
    fn row(&mut self, line: usize) -> &mut RowBuilder {
        match self.current {
            Some(i) if self.rows[i].total.is_none() => &mut self.rows[i],
            _ => {
                self.rows.push(RowBuilder {
                    line,
                    ..Default::default()
                });
                self.current = Some(self.rows.len() - 1);
                self.rows.last_mut().expect("just pushed")
            }
        }
    }

    fn heading(&mut self, h: &str, line: usize) {
        let (age_group, category, label) = parse_row_heading(h);
        self.rows.push(RowBuilder {
            line,
            total: None,
            label,
            age_group,
            category,
            headed: true,
        });
        self.current = Some(self.rows.len() - 1);
    }

    fn finish(mut self, errors: &mut Vec<ParseError>) -> Qualification {
        let (section_age, section_cat) = (self.age_group.clone(), self.category.clone());
        for row in self.rows {
            let Some(total) = row.total else {
                errors.push(err(row.line, "qualifying group has no '- needs: <total>'"));
                continue;
            };
            self.q.standards.push(Standard {
                total,
                label: row.label,
                age_group: row.age_group.or_else(|| section_age.clone()),
                category: row.category.or_else(|| section_cat.clone()),
            });
        }
        self.q
    }
}

fn qualification(b: &mut QualBuilder, key: &str, val: &str, line: usize, errors: &mut Vec<ParseError>) {
    let q = &mut b.q;
    let date = |slot: &mut Option<String>, field: &str, errors: &mut Vec<ParseError>| {
        if slot.is_some() {
            errors.push(err(line, format!("duplicate '{field}'")));
        } else if crate::days::valid_date(val) {
            *slot = Some(val.to_string());
        } else {
            errors.push(err(line, format!("invalid '{field}' date '{val}' — use YYYY-MM-DD")));
        }
    };
    match key {
        "from" | "since" => date(&mut q.from, "from", errors),
        "to" | "until" => date(&mut q.to, "to", errors),
        "counts" | "counts at" | "orgs" => q.counts.extend(list(val)),
        "needs" | "total" | "standard" => match parse_mark(val) {
            Some(t) => {
                let row = b.row(line);
                // A `###` heading has already named the row; a label on the
                // mark itself only speaks up when there is one.
                if !t.label.is_empty() || !row.headed {
                    row.label = t.label;
                }
                row.total = Some(t.total);
            }
            None => errors.push(err(
                line,
                format!("invalid mark '{val}' — write the total first, e.g. '250 A group'"),
            )),
        },
        "age" | "age group" => {
            let g = Some(val.to_string());
            match b.current {
                Some(i) if b.rows[i].total.is_none() => b.rows[i].age_group = g,
                _ => b.age_group = g,
            }
        }
        "category" | "class" | "bodyweight" => {
            let cat = Some(val.to_string());
            match b.current {
                Some(i) if b.rows[i].total.is_none() => b.rows[i].category = cat,
                _ => b.category = cat,
            }
        }
        _ => {}
    }
}

/// A comma-separated bullet value, e.g. `- org: BWL, IWF`.
fn list(val: &str) -> Vec<String> {
    val.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn preamble(c: &mut Competition, key: &str, val: &str, line: usize, errors: &mut Vec<ParseError>) {
    let dup = |field: &str, errors: &mut Vec<ParseError>| {
        errors.push(err(line, format!("duplicate '{field}'")));
    };
    match key {
        "id" => {
            if c.id.is_some() {
                dup("id", errors);
            } else if ids::valid_uuid(val) {
                c.id = Some(val.to_string());
            } else {
                errors.push(err(line, format!("invalid id '{val}' — expected a UUID")));
            }
        }
        "date" => {
            if c.date.is_some() {
                dup("date", errors);
            } else if crate::days::valid_date(val) {
                c.date = Some(val.to_string());
            } else {
                errors.push(err(line, format!("invalid date '{val}' — use YYYY-MM-DD")));
            }
        }
        "bodyweight" => {
            if c.bodyweight.is_some() {
                dup("bodyweight", errors);
            } else {
                match parse_kg(val.trim_end_matches("kg").trim()) {
                    Some(bw) => c.bodyweight = Some(bw),
                    None => errors.push(err(line, format!("invalid bodyweight '{val}'"))),
                }
            }
        }
        "category" => {
            if c.category.is_some() {
                dup("category", errors);
            } else {
                c.category = Some(val.to_string());
            }
        }
        "org" | "orgs" | "federation" | "sanctioned by" => c.orgs.extend(list(val)),
        "age group" | "age" => {
            if c.age_group.is_some() {
                dup("age group", errors);
            } else {
                c.age_group = Some(val.to_string());
            }
        }
        "target" => match parse_mark(val) {
            Some(t) => c.targets.push(t),
            None => errors.push(err(
                line,
                format!("invalid target '{val}' — write the total first, e.g. '230 today'"),
            )),
        },
        // `kind` is the routing marker, `updated` is written by the store;
        // anything else in the preamble is someone's own note.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "9f2c8e1a-4b7d-4c2e-9a11-6f0d3e5b8c74";

    const FULL: &str = "\
# Nationals 2026
- id: 9f2c8e1a-4b7d-4c2e-9a11-6f0d3e5b8c74
- kind: competition
- date: 2026-11-14
- bodyweight: 88.4
- category: 89 kg
- org: BWL, IWF
- age group: M40
- target: 230 today

## Qualification
- from: 2026-01-01
- to: 2026-10-31
- counts: BWL, FPH

### M35 89 kg
- needs: 215

### M40 89 kg
- needs: 205

## Snatch
- 1: 95 good
- 2: 99 miss
- 3: 99 good

### Warmup
- [x] 20 x 5
- [x] 60 x 2
- [ ] 85 x 1

Openers felt fast.

## Clean & Jerk
- 1: 120 good
- 2: 125
";

    fn made(kg: f64) -> Option<Attempt> {
        Some(Attempt {
            kg,
            result: AttemptResult::Good,
        })
    }

    fn missed(kg: f64) -> Option<Attempt> {
        Some(Attempt {
            kg,
            result: AttemptResult::Miss,
        })
    }

    fn declared(kg: f64) -> Option<Attempt> {
        Some(Attempt {
            kg,
            result: AttemptResult::Declared,
        })
    }

    fn comp(snatch: [Option<Attempt>; 3], cj: [Option<Attempt>; 3]) -> Competition {
        let mut c = Competition::new("Meet");
        c.snatch.attempts = snatch;
        c.clean_jerk.attempts = cj;
        c
    }

    #[test]
    fn parses_full_document() {
        let c = parse_competition(FULL).unwrap();
        assert_eq!(c.name, "Nationals 2026");
        assert_eq!(c.id.as_deref(), Some(UUID));
        assert_eq!(c.date.as_deref(), Some("2026-11-14"));
        assert_eq!(c.bodyweight, Some(88.4));
        assert_eq!(c.category.as_deref(), Some("89 kg"));
        assert_eq!(c.orgs, vec!["BWL", "IWF"]);
        assert_eq!(c.age_group.as_deref(), Some("M40"));
        assert_eq!(c.targets.len(), 1);
        assert_eq!(c.targets[0].total, 230.0);
        assert_eq!(c.targets[0].label, "today");
        let q = c.qualification.clone().unwrap();
        assert_eq!(q.from.as_deref(), Some("2026-01-01"));
        assert_eq!(q.to.as_deref(), Some("2026-10-31"));
        assert_eq!(q.counts, vec!["BWL", "FPH"]);
        assert_eq!(q.standards.len(), 2);
        assert_eq!(q.standards[1].total, 205.0);
        assert_eq!(q.standards[1].age_group.as_deref(), Some("M40"));
        assert_eq!(q.standards[1].category.as_deref(), Some("89"));
        assert_eq!(c.snatch.attempts[1], missed(99.0));
        assert_eq!(c.snatch.warmup.len(), 3);
        assert_eq!(
            c.snatch.warmup[0],
            WarmupSet {
                kg: 20.0,
                reps: 5,
                done: true
            }
        );
        assert!(!c.snatch.warmup[2].done);
        assert_eq!(c.snatch.notes_md, "Openers felt fast.");
        assert_eq!(c.clean_jerk.attempts[1], declared(125.0));
        assert_eq!(c.clean_jerk.attempts[2], None);
    }

    #[test]
    fn round_trips() {
        let c = parse_competition(FULL).unwrap();
        let md = competition_to_markdown(&c);
        assert_eq!(parse_competition(&md).unwrap(), c);
    }

    #[test]
    fn a_meet_with_nothing_declared_is_a_valid_document() {
        // Entering a competition comes long before declaring an opener.
        let c = parse_competition("# Club Open\n- kind: competition\n").unwrap();
        assert_eq!(c.snatch.attempts, [None, None, None]);
        assert_eq!(c.total(), TotalState::Open);
    }

    #[test]
    fn a_title_is_required() {
        let errs = parse_competition("- kind: competition\n").unwrap_err();
        assert!(errs[0].message.contains("missing competition title"));
    }

    #[test]
    fn the_clean_and_jerk_may_be_spelled_any_way() {
        for h in ["Clean & Jerk", "clean and jerk", "C&J", "CJ", "Clean-Jerk"] {
            assert_eq!(Lift::from_heading(h), Some(Lift::CleanJerk), "{h}");
        }
    }

    #[test]
    fn an_unknown_section_is_an_error_rather_than_dropped() {
        let errs = parse_competition("# M\n\n## Back Squat\n- 1: 200 good\n").unwrap_err();
        assert_eq!(errs[0].line, 3);
        assert!(errs[0].message.contains("unknown section"), "{}", errs[0].message);
    }

    #[test]
    fn a_meet_you_have_not_been_to_is_a_document_of_its_own() {
        // Europeans, entered as the thing you are chasing: no attempts, just
        // what it would take to be there.
        let c = parse_competition(
            "# Europeans 2027\n- kind: competition\n- date: 2027-04-10\n\n## Qualification\n- needs: 250\n",
        )
        .unwrap();
        assert_eq!(c.total(), TotalState::Open);
        let q = c.qualification.unwrap();
        assert_eq!(q.standards[0].total, 250.0);
        assert_eq!(q.standards[0].label, "");
    }

    #[test]
    fn a_fourth_attempt_is_an_error() {
        let errs = parse_competition("# M\n\n## Snatch\n- 4: 100\n").unwrap_err();
        assert!(errs[0].message.contains("attempts 1 to 3"), "{}", errs[0].message);
    }

    #[test]
    fn an_unknown_result_word_is_an_error() {
        // Better to say so than to file it as "declared" and show a weight
        // the lifter has already taken as one still to come.
        let errs = parse_competition("# M\n\n## Snatch\n- 1: 100 maybe\n").unwrap_err();
        assert!(errs[0].message.contains("unknown result"), "{}", errs[0].message);
    }

    #[test]
    fn prose_in_a_section_is_kept_as_notes() {
        let c = parse_competition("# M\n\n## Snatch\n- 1: 100\n\nHit the pull.\n").unwrap();
        assert_eq!(c.snatch.notes_md, "Hit the pull.");
    }

    #[test]
    fn warmup_reps_default_to_one() {
        let c = parse_competition("# M\n\n## Snatch\n- [ ] 85\n").unwrap();
        assert_eq!(c.snatch.warmup[0].reps, 1);
    }

    #[test]
    fn half_kilo_warmups_survive_the_round_trip() {
        let c = parse_competition("# M\n\n## Snatch\n- [x] 42.5 x 2\n").unwrap();
        assert_eq!(c.snatch.warmup[0].kg, 42.5);
        let back = parse_competition(&competition_to_markdown(&c)).unwrap();
        assert_eq!(back.snatch.warmup[0].kg, 42.5);
    }

    #[test]
    fn only_a_kind_bullet_makes_a_document_a_competition() {
        assert!(looks_like_competition(FULL));
        assert!(!looks_like_competition("# W\n\n## A\n- work: 30\n"));
        // A workout whose notes mention the word must not be mistaken for one.
        assert!(!looks_like_competition("# W\n\n## A\n- kind: competition\n"));
    }

    #[test]
    fn weights_are_written_the_way_they_are_read() {
        assert_eq!(fmt_kg(95.0), "95");
        assert_eq!(fmt_kg(42.5), "42.5");
        assert_eq!(fmt_kg(88.4), "88.4");
    }

    // ---- the arithmetic ----

    #[test]
    fn only_good_lifts_count() {
        let c = comp([made(95.0), missed(99.0), made(97.0)], [made(120.0), None, None]);
        assert_eq!(c.snatch.best(), Some(97.0));
        assert_eq!(c.total(), TotalState::Made { total: 217.0 });
    }

    #[test]
    fn there_is_no_total_until_both_lifts_are_on_the_board() {
        let c = comp([made(95.0), None, None], [declared(120.0), None, None]);
        assert_eq!(c.total(), TotalState::Open);
        assert_eq!(c.total().kg(), None);
    }

    #[test]
    fn three_misses_is_a_bomb_out_not_a_zero() {
        let c = comp(
            [missed(95.0), missed(95.0), missed(95.0)],
            [made(120.0), None, None],
        );
        assert_eq!(c.total(), TotalState::BombedOut);
        assert!(c.snatch.bombed_out());
    }

    #[test]
    fn best_possible_counts_what_is_declared() {
        let c = comp(
            [made(95.0), missed(99.0), declared(99.0)],
            [made(120.0), declared(126.0), None],
        );
        assert_eq!(c.snatch.best_possible(), Some(99.0));
        assert_eq!(c.best_possible_total(), Some(225.0));
    }

    #[test]
    fn best_possible_never_drops_below_what_is_already_made() {
        // A third attempt declared lighter than a good second one cannot make
        // the projection go backwards.
        let c = comp([made(100.0), None, declared(96.0)], [None, None, None]);
        assert_eq!(c.snatch.best_possible(), Some(100.0));
    }

    #[test]
    fn the_bar_goes_up_after_a_good_lift_and_may_be_repeated_after_a_miss() {
        assert_eq!(entry([made(95.0), None, None]).min_next_kg(), Some(96.0));
        assert_eq!(entry([missed(95.0), None, None]).min_next_kg(), Some(95.0));
        assert_eq!(LiftEntry::default().min_next_kg(), None);
    }

    #[test]
    fn next_attempt_skips_what_is_spent() {
        assert_eq!(LiftEntry::default().next_attempt(), Some(1));
        assert_eq!(entry([made(95.0), None, None]).next_attempt(), Some(2));
        // A declared third is still to come; a taken one is not.
        assert_eq!(
            entry([made(95.0), missed(99.0), declared(99.0)]).next_attempt(),
            Some(3)
        );
        let spent = entry([made(95.0), missed(99.0), made(99.0)]);
        assert_eq!(spent.next_attempt(), None);
        assert_eq!(spent.attempts_left(), 0);
    }

    fn entry(attempts: [Option<Attempt>; ATTEMPTS]) -> LiftEntry {
        LiftEntry {
            attempts,
            ..Default::default()
        }
    }

    #[test]
    fn a_bar_that_goes_down_is_flagged() {
        let down = entry([made(95.0), declared(93.0), declared(100.0)]);
        assert_eq!(down.attempts_going_down(), vec![2]);
        let up = entry([made(95.0), declared(99.0), declared(103.0)]);
        assert!(up.attempts_going_down().is_empty());
    }

    #[test]
    fn a_target_already_made_is_clinched() {
        let c = comp([made(100.0), None, None], [made(130.0), None, None]);
        assert_eq!(c.target_status(230.0), TargetStatus::Clinched);
    }

    #[test]
    fn a_target_comes_down_to_one_attempt_once_a_lift_is_banked() {
        let c = comp(
            [made(95.0), made(99.0), missed(103.0)],
            [made(120.0), missed(126.0), None],
        );
        assert_eq!(
            c.target_status(240.0),
            TargetStatus::Needs {
                lift: Lift::CleanJerk,
                attempt: 3,
                kg: 141.0,
            }
        );
    }

    #[test]
    fn what_a_target_needs_is_never_below_what_the_bar_is_already_at() {
        // 120 would do it, but the clean & jerk is already loaded to 126 after
        // a miss — there is no going back down.
        let c = comp(
            [made(100.0), None, None],
            [missed(126.0), None, None],
        );
        assert_eq!(
            c.target_status(215.0),
            TargetStatus::Needs {
                lift: Lift::CleanJerk,
                attempt: 2,
                kg: 126.0,
            }
        );
    }

    #[test]
    fn a_target_stays_open_while_both_lifts_are_unfinished() {
        // During the snatch, what the bar needs to be depends on a clean &
        // jerk that has not happened. Saying a number here would be a guess.
        let c = comp([made(95.0), None, None], [None, None, None]);
        assert_eq!(c.target_status(240.0), TargetStatus::Open);
    }

    #[test]
    fn a_bombed_lift_puts_every_target_out_of_reach() {
        let c = comp(
            [missed(95.0), missed(95.0), missed(95.0)],
            [made(130.0), None, None],
        );
        assert_eq!(c.target_status(200.0), TargetStatus::OutOfReach);
    }

    #[test]
    fn a_finished_meet_below_the_target_is_out_of_reach() {
        let c = comp(
            [made(95.0), missed(99.0), missed(99.0)],
            [made(120.0), missed(126.0), missed(126.0)],
        );
        assert_eq!(c.target_status(240.0), TargetStatus::OutOfReach);
        assert_eq!(c.target_status(215.0), TargetStatus::Clinched);
    }

    #[test]
    fn a_remaining_attempt_keeps_a_target_alive_however_big_it_is() {
        // Nothing caps a declaration, so nothing is out of reach while an
        // attempt is in hand — only a proof counts.
        let c = comp([made(95.0), missed(99.0), missed(99.0)], [made(120.0), None, None]);
        assert!(matches!(
            c.target_status(400.0),
            TargetStatus::Needs { .. }
        ));
    }

    // ---- what counts toward what ----

    fn result(date: &str, org: &str, snatch: f64, cj: f64) -> Competition {
        let mut c = comp([made(snatch), None, None], [made(cj), None, None]);
        c.date = Some(date.to_string());
        c.orgs = list(org);
        c
    }

    fn meet_at(date: &str, org: &str) -> Competition {
        let mut c = Competition::new("Today");
        c.date = Some(date.into());
        c.orgs = list(org);
        c
    }

    fn window(from: &str, to: &str, counts: &[&str], needs: f64) -> Qualification {
        Qualification {
            from: Some(from.into()),
            to: Some(to.into()),
            counts: counts.iter().map(|s| s.to_string()).collect(),
            standards: vec![Standard {
                total: needs,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn a_result_outside_the_window_does_not_count() {
        let q = window("2026-01-01", "2026-10-31", &["IWF"], 250.0);
        assert!(q.accepts(&result("2026-03-02", "IWF", 110.0, 140.0)));
        assert!(!q.accepts(&result("2025-12-31", "IWF", 110.0, 140.0)));
        assert!(!q.accepts(&result("2026-11-01", "IWF", 110.0, 140.0)));
    }

    #[test]
    fn a_result_from_the_wrong_org_does_not_count() {
        let q = window("2026-01-01", "2026-10-31", &["IWF", "EWF"], 250.0);
        assert!(q.accepts(&result("2026-03-02", "ewf", 110.0, 140.0)));
        assert!(!q.accepts(&result("2026-03-02", "Club Open", 110.0, 140.0)));
        // No list named means no restriction — a mark you set yourself.
        let open = Qualification {
            counts: vec![],
            ..window("2026-01-01", "2026-10-31", &[], 250.0)
        };
        assert!(open.accepts(&result("2026-03-02", "Club Open", 110.0, 140.0)));
    }

    #[test]
    fn an_undated_result_fails_a_window_rather_than_passing_it() {
        let q = window("2026-01-01", "2026-10-31", &[], 250.0);
        let mut meet = result("2026-03-02", "IWF", 110.0, 140.0);
        meet.date = None;
        assert!(!q.accepts(&meet));
        // With no window to check it against, the same meet is fine.
        assert!(Qualification::default().accepts(&meet));
    }

    #[test]
    fn a_big_enough_total_at_a_meet_that_counts_is_the_qualification() {
        let q = window("2026-01-01", "2026-10-31", &["IWF"], 250.0);
        let standard = &q.standards[0];
        let too_early = result("2025-06-01", "IWF", 115.0, 145.0);
        let wrong_org = result("2026-06-01", "Club Open", 115.0, 145.0);
        let too_light = result("2026-06-01", "IWF", 105.0, 135.0);
        let the_one = result("2026-07-04", "IWF", 112.0, 140.0);
        let results = vec![too_early, wrong_org, too_light.clone(), the_one];
        assert_eq!(
            q.met_by(standard, &results).and_then(|m| m.date.clone()),
            Some("2026-07-04".into())
        );
        assert!(q.met_by(standard, &[too_light]).is_none());
    }

    #[test]
    fn only_the_doors_todays_meet_can_open_are_in_play() {
        let mut europeans = Competition::new("Europeans 2027");
        europeans.qualification = Some(window("2026-01-01", "2026-10-31", &["IWF"], 250.0));
        let mut worlds = Competition::new("Worlds 2027");
        worlds.qualification = Some(window("2026-01-01", "2026-10-31", &["IWF"], 280.0));
        let mut nationals = Competition::new("Nationals 2027");
        // Its window closed before today's meet.
        nationals.qualification = Some(window("2025-01-01", "2025-12-31", &["IWF"], 230.0));
        let chasing = vec![europeans, worlds, nationals];

        let today = meet_at("2026-06-01", "IWF");
        let in_play = marks_in_play(&today, &chasing, &[]);
        assert_eq!(in_play.len(), 2);
        assert_eq!(in_play[0].0.name, "Europeans 2027");
        assert_eq!(in_play[1].0.name, "Worlds 2027");

        // Europeans is already in the bag, so it is no longer something to
        // lift for today.
        let done = vec![result("2026-02-01", "IWF", 115.0, 140.0)];
        let in_play = marks_in_play(&today, &chasing, &done);
        assert_eq!(in_play.len(), 1);
        assert_eq!(in_play[0].0.name, "Worlds 2027");
    }

    #[test]
    fn a_meet_answering_to_two_federations_counts_for_both() {
        // An international held in Portugal is the thing that can qualify you
        // for something in England — which only works if a meet is allowed to
        // name more than one body.
        let lisbon = result("2026-05-10", "FPH, IWF", 112.0, 140.0);
        let english = window("2026-01-01", "2026-12-31", &["BWL", "IWF"], 250.0);
        let portuguese = window("2026-01-01", "2026-12-31", &["FPH"], 250.0);
        assert!(english.accepts(&lisbon));
        assert!(portuguese.accepts(&lisbon));
        // A club meet under neither is still just a club meet.
        let club = result("2026-05-10", "Club Open", 112.0, 140.0);
        assert!(!english.accepts(&club));
    }

    #[test]
    fn an_age_group_is_the_same_group_however_it_is_written() {
        for (a, b) in [
            ("M40", "40-44"),
            ("M40", "m40"),
            ("Masters 40", "M40"),
            ("40-44", "40"),
            ("Senior", "senior"),
        ] {
            assert!(age_groups_match(a, b), "{a} vs {b}");
        }
        for (a, b) in [("M40", "M45"), ("M40", "W40"), ("Senior", "Junior")] {
            assert!(!age_groups_match(a, b), "{a} vs {b}");
        }
    }

    #[test]
    fn a_bodyweight_class_is_the_same_class_however_it_is_written() {
        for (a, b) in [("89", "89 kg"), ("89kg", "-89"), ("+89", "+89 kg")] {
            assert!(categories_match(a, b), "{a} vs {b}");
        }
        // The open class above a weight is not the class up to it.
        assert!(!categories_match("+89", "89"));
        assert!(!categories_match("89", "96"));
    }

    #[test]
    fn a_row_heading_says_which_group_the_mark_is_for() {
        assert_eq!(
            parse_row_heading("M40 89 kg"),
            (Some("M40".into()), Some("89".into()), String::new())
        );
        assert_eq!(
            parse_row_heading("40-44 +89 kg A group"),
            (Some("40-44".into()), Some("+89".into()), "A group".into())
        );
        // Nothing that names a group: the whole heading is just its name.
        assert_eq!(
            parse_row_heading("A group"),
            (None, None, "A group".into())
        );
    }

    #[test]
    fn a_masters_table_gives_you_your_own_row() {
        let q = parse_competition(FULL).unwrap().qualification.unwrap();
        let mine = q.standard_for(Some("M40"), Some("89 kg")).unwrap();
        assert_eq!(mine.total, 205.0);
        let younger = q.standard_for(Some("M35"), Some("89")).unwrap();
        assert_eq!(younger.total, 215.0);
        // A group with no row of its own gets no number, rather than someone
        // else's.
        assert!(q.standard_for(Some("M50"), Some("89")).is_none());
        assert!(q.standard_for(Some("M40"), Some("96")).is_none());
        // Say nothing about who you are and you get the whole table.
        assert_eq!(q.applicable(None, None).len(), 2);
    }

    #[test]
    fn the_narrowest_row_wins() {
        let q = Qualification {
            standards: vec![
                Standard {
                    total: 250.0,
                    ..Default::default()
                },
                Standard {
                    total: 205.0,
                    age_group: Some("M40".into()),
                    category: Some("89".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(q.standard_for(Some("M40"), Some("89 kg")).unwrap().total, 205.0);
        // Another lifter falls back to the mark that names nobody.
        assert_eq!(q.standard_for(Some("M60"), Some("96")).unwrap().total, 250.0);
    }

    #[test]
    fn a_section_wide_group_qualifies_every_row_under_it() {
        // The natural way to write a table that is all one category.
        let c = parse_competition(
            "# Nationals\n\n## Qualification\n- category: 89 kg\n- needs: 250 A group\n- needs: 240 B group\n",
        )
        .unwrap();
        let q = c.qualification.unwrap();
        assert_eq!(q.standards.len(), 2);
        assert!(q.standards.iter().all(|s| s.category.as_deref() == Some("89 kg")));
        assert_eq!(q.standards[1].label, "B group");
        assert!(q.standard_for(None, Some("96")).is_none());
    }

    #[test]
    fn a_group_with_no_mark_under_it_is_an_error() {
        // Silently dropping the row would leave a qualification that quietly
        // asks for nothing.
        let errs =
            parse_competition("# N\n\n## Qualification\n\n### M40 89 kg\n\n### M45 89 kg\n- needs: 195\n")
                .unwrap_err();
        assert!(errs[0].message.contains("no '- needs:"), "{}", errs[0].message);
    }

    #[test]
    fn marks_are_picked_for_the_entry_you_intend_at_the_meet_you_are_chasing() {
        let mut british = Competition::new("British Masters 2027");
        british.age_group = Some("M40".into());
        british.category = Some("89 kg".into());
        british.qualification = Some(Qualification {
            counts: vec!["BWL".into()],
            standards: vec![
                Standard {
                    total: 205.0,
                    age_group: Some("M40".into()),
                    category: Some("89".into()),
                    ..Default::default()
                },
                Standard {
                    total: 215.0,
                    age_group: Some("M35".into()),
                    category: Some("89".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let chasing = vec![british];
        let today = meet_at("2026-06-01", "BWL");
        let in_play = marks_in_play(&today, &chasing, &[]);
        assert_eq!(in_play.len(), 1);
        assert_eq!(in_play[0].1.total, 205.0);
    }

    #[test]
    fn a_qualification_round_trips() {
        let c = parse_competition(FULL).unwrap();
        let back = parse_competition(&competition_to_markdown(&c)).unwrap();
        assert_eq!(back.qualification, c.qualification);
        assert_eq!(back.orgs, c.orgs);
        assert_eq!(back.age_group, c.age_group);
    }

    #[test]
    fn sinclair_scales_a_lighter_lifter_up_and_leaves_the_heaviest_alone() {
        let c = SinclairCoefficients {
            a: 0.722_762_005,
            b: 193.609_867,
        };
        let light = sinclair(220.0, 89.0, c).unwrap();
        assert!(light > 220.0, "{light}");
        assert_eq!(sinclair(220.0, 200.0, c), Some(220.0));
        assert_eq!(sinclair(0.0, 89.0, c), None);
    }
}
