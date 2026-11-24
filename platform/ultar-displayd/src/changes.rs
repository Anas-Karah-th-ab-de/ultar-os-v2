//! `display.apply`: what each change means for the settings of `settingsd`.
//! The settings service makes them provisional ("Keep?", 15 s); this module
//! only checks the change against the connected displays and names keys.

use crate::plan::{main_display, Connected};
use crate::values::DisplayValues;
use ultar_bus::messages::display_service::{code, Change, Projection};
use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::OutputMode;

/// Largest position accepted (logical points).
const MAX_POSITION: i32 = 32_768;

/// Why a change was refused (`ErrorCode::Service(code())`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeError {
    /// A display id that is not connected.
    UnknownDisplay,
    /// A change that does not fit the displays.
    Invalid,
}

impl ChangeError {
    /// The service error code.
    pub fn code(self) -> u32 {
        match self {
            ChangeError::UnknownDisplay => code::UNKNOWN_DISPLAY,
            ChangeError::Invalid => code::INVALID,
        }
    }
}

impl std::fmt::Display for ChangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ChangeError::UnknownDisplay => "display not connected",
            ChangeError::Invalid => "change does not fit the displays",
        })
    }
}

impl std::error::Error for ChangeError {}

fn text(t: &str) -> SettingValue {
    SettingValue::Text(t.to_owned())
}

fn find<'a>(connected: &'a [Connected], id: &str) -> Result<&'a Connected, ChangeError> {
    connected
        .iter()
        .find(|c| c.id == id)
        .ok_or(ChangeError::UnknownDisplay)
}

/// The display Super+P means by "only this device": the built-in one, else
/// the main one; by "only the second": the first other external display.
fn projection(
    connected: &[Connected],
    values: &DisplayValues,
    p: Projection,
) -> Result<Vec<(String, SettingValue)>, ChangeError> {
    if connected.len() < 2 {
        return Err(ChangeError::Invalid);
    }
    let own = connected
        .iter()
        .find(|c| c.details.internal)
        .or_else(|| main_display(connected, values))
        .ok_or(ChangeError::Invalid)?;
    let single = |c: &Connected| {
        vec![
            ("display.mode".to_owned(), text("single")),
            ("display.single_display".to_owned(), text(&c.id)),
        ]
    };
    Ok(match p {
        Projection::ThisDeviceOnly => single(own),
        Projection::SecondOnly => {
            let second = connected
                .iter()
                .find(|c| c.id != own.id && !c.details.internal)
                .or_else(|| connected.iter().find(|c| c.id != own.id))
                .ok_or(ChangeError::Invalid)?;
            single(second)
        }
        Projection::Mirror => vec![("display.mode".to_owned(), text("mirror"))],
        Projection::Extend => vec![("display.mode".to_owned(), text("extend"))],
    })
}

/// The settings a change sets, in order.
pub fn settings_for(
    change: &Change,
    connected: &[Connected],
    values: &DisplayValues,
) -> Result<Vec<(String, SettingValue)>, ChangeError> {
    match change {
        Change::Projection(p) => projection(connected, values, *p),
        Change::Primary(id) => {
            let c = find(connected, id)?;
            Ok(vec![("display.primary".to_owned(), text(&c.id))])
        }
        Change::Arrangement(list) => {
            if list.is_empty() || list.len() > connected.len() {
                return Err(ChangeError::Invalid);
            }
            let mut out = Vec::new();
            for (id, x, y) in list {
                let c = find(connected, id)?;
                if x.abs() > MAX_POSITION || y.abs() > MAX_POSITION {
                    return Err(ChangeError::Invalid);
                }
                out.push((
                    format!("display.{}.position_x", c.id),
                    SettingValue::Integer((*x).into()),
                ));
                out.push((
                    format!("display.{}.position_y", c.id),
                    SettingValue::Integer((*y).into()),
                ));
            }
            Ok(out)
        }
        Change::Mode { id, mode } => {
            let c = find(connected, id)?;
            let wanted = OutputMode {
                width: mode.width,
                height: mode.height,
                refresh_mhz: mode.refresh_mhz,
            };
            if !c.details.modes.contains(&wanted) {
                return Err(ChangeError::Invalid);
            }
            Ok(vec![
                (
                    format!("display.{}.resolution", c.id),
                    text(&format!("{}x{}", mode.width, mode.height)),
                ),
                (
                    format!("display.{}.refresh_rate", c.id),
                    SettingValue::Decimal(f64::from(mode.refresh_mhz) / 1000.0),
                ),
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::display_service::Mode;
    use ultar_surface::proto::{OutputDetails, OutputId};

    fn c(i: u32, internal: bool) -> Connected {
        let mode = OutputMode {
            width: 1920,
            height: 1080,
            refresh_mhz: 60_000,
        };
        Connected {
            details: OutputDetails {
                id: OutputId(i),
                internal,
                preferred: mode,
                current: mode,
                modes: vec![mode],
                ..OutputDetails::default()
            },
            id: format!("d{i}"),
        }
    }

    fn keys(v: &[(String, SettingValue)]) -> Vec<(&str, &SettingValue)> {
        v.iter().map(|(k, v)| (k.as_str(), v)).collect()
    }

    #[test]
    fn super_p_names_the_right_displays() {
        let three = [c(0, false), c(1, true), c(2, false)];
        let v = DisplayValues::default();
        let own =
            settings_for(&Change::Projection(Projection::ThisDeviceOnly), &three, &v).unwrap();
        assert_eq!(keys(&own)[1], ("display.single_display", &text("d1")));
        let second = settings_for(&Change::Projection(Projection::SecondOnly), &three, &v).unwrap();
        assert_eq!(
            keys(&second)[1],
            ("display.single_display", &text("d0")),
            "first external"
        );
        let mirror = settings_for(&Change::Projection(Projection::Mirror), &three, &v).unwrap();
        assert_eq!(keys(&mirror), vec![("display.mode", &text("mirror"))]);
        let extend = settings_for(&Change::Projection(Projection::Extend), &three, &v).unwrap();
        assert_eq!(keys(&extend), vec![("display.mode", &text("extend"))]);
        assert_eq!(
            settings_for(&Change::Projection(Projection::Extend), &three[..1], &v),
            Err(ChangeError::Invalid)
        );
        let two_external = [c(0, false), c(1, false)];
        let s = settings_for(
            &Change::Projection(Projection::SecondOnly),
            &two_external,
            &v,
        )
        .unwrap();
        assert_eq!(keys(&s)[1], ("display.single_display", &text("d1")));
    }

    #[test]
    fn primary_positions_and_modes_are_checked() {
        let two = [c(0, true), c(1, false)];
        let v = DisplayValues::default();
        assert_eq!(
            settings_for(&Change::Primary("d1".into()), &two, &v)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            settings_for(&Change::Primary("nope".into()), &two, &v),
            Err(ChangeError::UnknownDisplay)
        );
        let moved = settings_for(
            &Change::Arrangement(vec![("d1".into(), -1920, 40)]),
            &two,
            &v,
        )
        .unwrap();
        assert_eq!(
            moved[0],
            ("display.d1.position_x".into(), SettingValue::Integer(-1920))
        );
        assert_eq!(
            settings_for(
                &Change::Arrangement(vec![("d1".into(), 90_000, 0)]),
                &two,
                &v
            ),
            Err(ChangeError::Invalid)
        );
        assert_eq!(
            settings_for(&Change::Arrangement(vec![]), &two, &v),
            Err(ChangeError::Invalid)
        );
    }

    #[test]
    fn modes_must_be_offered() {
        let two = [c(0, true), c(1, false)];
        let v = DisplayValues::default();
        let mode = Mode {
            width: 1920,
            height: 1080,
            refresh_mhz: 60_000,
        };
        let set = settings_for(
            &Change::Mode {
                id: "d0".into(),
                mode,
            },
            &two,
            &v,
        )
        .unwrap();
        assert_eq!(set[0].1, text("1920x1080"));
        assert_eq!(set[1].1, SettingValue::Decimal(60.0));
        let odd = Mode { width: 7, ..mode };
        assert_eq!(
            settings_for(
                &Change::Mode {
                    id: "d0".into(),
                    mode: odd
                },
                &two,
                &v
            ),
            Err(ChangeError::Invalid)
        );
        assert_eq!(ChangeError::UnknownDisplay.code(), code::UNKNOWN_DISPLAY);
        assert!(!ChangeError::Invalid.to_string().is_empty());
    }
}
