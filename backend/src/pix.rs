const GUI: &str = "br.gov.bcb.pix";
const CRC_POLY: u16 = 0x1021;

pub fn copia_e_cola(txid: &str, name: &str, city: &str, chave: &str, amount_cents: i64) -> String {
    let amount = format!("{}.{:02}", amount_cents / 100, amount_cents % 100);
    let merchant = format!("{}{}", tlv("00", GUI), tlv("01", chave));
    let additional = tlv("05", txid);
    let mut payload = String::new();
    payload.push_str(&tlv("00", "01"));
    payload.push_str(&tlv("26", &merchant));
    payload.push_str(&tlv("52", "0000"));
    payload.push_str(&tlv("53", "986"));
    payload.push_str(&tlv("54", &amount));
    payload.push_str(&tlv("58", "BR"));
    payload.push_str(&tlv("59", &clip(name, 25)));
    payload.push_str(&tlv("60", &clip(city, 15)));
    payload.push_str(&tlv("62", &additional));
    payload.push_str("6304");
    let crc = crc16(&payload);
    payload.push_str(&crc);
    payload
}

fn tlv(id: &str, value: &str) -> String {
    format!("{id}{:02}{value}", value.len())
}

fn clip(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > n {
        chars[..n].iter().collect()
    } else if s.is_empty() {
        "NA".into()
    } else {
        s.to_string()
    }
}

fn crc16(s: &str) -> String {
    let mut crc: u16 = 0xFFFF;
    for b in s.bytes() {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ CRC_POLY;
            } else {
                crc <<= 1;
            }
        }
    }
    format!("{crc:04X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copia_e_cola_has_crc_and_amount() {
        let code = copia_e_cola(
            "ord_test",
            "Arena Store Demo",
            "SAO PAULO",
            "arena@demo.local",
            19990,
        );
        assert!(code.starts_with("000201"), "{code}");
        assert!(code.contains("5406199.90"), "{code}");
        assert!(code.contains("br.gov.bcb.pix"), "{code}");
        let body = &code[..code.len() - 4];
        assert_eq!(&code[code.len() - 4..], crc16(body));
    }
}
