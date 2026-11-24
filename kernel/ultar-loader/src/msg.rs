//! The loader's messages (firmware console, Arabic first). The firmware console may not draw
//! Arabic letters; the English and German lines carry the same text.

/// No slot can be started (both damaged, or the table is unreadable).
pub const NO_SLOT: &str = concat!(
    "\r\n",
    ultar_brand::product!(),
    ": لا يمكن بدء أي نسخة من النظام – صِل وسيط التثبيت.\r\n",
    ultar_brand::product!(),
    ": no system slot can be started - connect the installation medium.\r\n",
    ultar_brand::product!(),
    ": kein Systembereich startbar - bitte das Installationsmedium anschliessen.\r\n"
);
/// The start partition is not on a disk with the A/B layout.
pub const NO_LAYOUT: &str = concat!(
    "\r\n",
    ultar_brand::product!(),
    ": لم يُعثر على بيانات الإقلاع (ULTAR-BOOTCTL).\r\n",
    ultar_brand::product!(),
    ": boot control data (ULTAR-BOOTCTL) not found.\r\n",
    ultar_brand::product!(),
    ": Startdaten (ULTAR-BOOTCTL) nicht gefunden.\r\n"
);
/// The rescue program is started.
pub const RESCUE: &str = concat!(
    "\r\n",
    ultar_brand::product!(),
    ": بدء وضع الإنقاذ\r\n",
    ultar_brand::product!(),
    ": starting rescue mode\r\n",
    ultar_brand::product!(),
    ": Rettungsmodus startet\r\n"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_starts_with_the_product_name_of_the_brand_source() {
        for message in [NO_SLOT, NO_LAYOUT, RESCUE] {
            let lines: Vec<&str> = message.lines().filter(|l| !l.is_empty()).collect();
            // Arabic first, then English and German
            assert_eq!(lines.len(), 3);
            for line in lines {
                let rest = line.strip_prefix(ultar_brand::PRODUCT);
                assert!(rest.is_some_and(|rest| rest.starts_with(": ")), "{line}");
            }
            assert!(message.starts_with("\r\n") && message.ends_with("\r\n"));
        }
    }
}
