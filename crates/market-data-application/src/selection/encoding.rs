use super::*;

// Private fixed-width, length-delimited encoding. Include values as well as caller-supplied
// checksums so accidentally reused witnesses cannot hide changed selection/directory content.
pub(super) fn binding(plan: &SelectionPlan<'_>) -> Result<Vec<u8>> {
    let mut out = b"edgeagent.selection.v1\0".to_vec();
    macro_rules! number {
        ($v:expr) => {
            out.extend_from_slice(&($v).to_be_bytes())
        };
    }
    macro_rules! text {
        ($s:expr) => {{
            let s = $s;
            number!(s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }};
    }
    macro_rules! witness {
        ($v:expr) => {{
            let v = $v;
            number!(v.id().get());
            number!(v.revision().get());
            number!(v.source().get());
            number!(v.known_at().unix_millis());
            out.extend_from_slice(v.checksum());
        }};
    }
    macro_rules! instant_range {
        ($r:expr) => {{
            let (a, b) = $r.bounds();
            number!(a.unix_millis());
            number!(b.unix_millis());
        }};
    }
    macro_rules! date {
        ($d:expr) => {{
            let (y, m, d) = $d.ymd();
            number!(y);
            number!(m);
            number!(d);
        }};
    }
    let context = plan.context;
    number!(context.scope.source.get());
    number!(context.scope.feed.get());
    number!(context.scope.epoch.get());
    number!(context.effective_at.unix_millis());
    number!(context.cutoff.unix_millis());
    date!(context.session);
    witness!(plan.selection);
    instant_range!(plan.selection.value().effective);
    number!(plan.selection.value().members.len() as u64);
    for member in &plan.selection.value().members {
        number!(member.get());
    }
    witness!(plan.directory);
    instant_range!(plan.directory.value().effective);
    number!(plan.directory.value().entries.len() as u64);
    for (key, mapping) in &plan.directory.value().entries {
        match &key.0 {
            Key::Symbol(s) => {
                out.push(0);
                text!(s);
            }
            Key::Locate(n) => {
                out.push(1);
                number!(n.get());
            }
        }
        witness!(mapping);
        let (instrument, listing) = mapping.value().identities();
        number!(instrument.get());
        number!(listing.get());
        let (symbol, venue, range) = mapping.value().binding();
        text!(symbol.as_str());
        out.push(match venue {
            Venue::Xnys => 0,
            Venue::Xnas => 1,
            Venue::Arcx => 2,
            Venue::Xiex => 3,
            _ => return Err(SelectionError::Scope),
        });
        let (a, b) = range.bounds();
        date!(a);
        date!(b);
    }
    Ok(out)
}
