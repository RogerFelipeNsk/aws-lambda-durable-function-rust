use crate::domain::User;
use crate::id;
use chrono::Utc;

const FIRST_NAMES: &[&str] = &[
    "Ana", "Bruno", "Camila", "Diego", "Elisa", "Felipe", "Giovana", "Henrique", "Isabela", "João",
    "Karina", "Lucas", "Marina", "Nicolas", "Olivia", "Paulo", "Queila", "Rafael", "Sofia",
    "Tiago", "Úrsula", "Vitor", "Wendy", "Yasmin",
];

const LAST_NAMES: &[&str] = &[
    "Almeida",
    "Barbosa",
    "Cardoso",
    "Dias",
    "Esteves",
    "Ferreira",
    "Gomes",
    "Henriques",
    "Ibrahim",
    "Jesus",
    "Lima",
    "Mendes",
    "Nogueira",
    "Oliveira",
    "Pereira",
    "Rocha",
    "Santos",
    "Teixeira",
    "Uchoa",
    "Vieira",
];

pub fn new_user() -> User {
    let first = FIRST_NAMES[pick(FIRST_NAMES.len())];
    let last = LAST_NAMES[pick(LAST_NAMES.len())];
    let uid = id::new("usr");
    User {
        id: uid.clone(),
        name: format!("{first} {last}"),
        email: format!("visitante.{uid}@arena-demo.local"),
        created_at: Utc::now(),
    }
}

fn pick(n: usize) -> usize {
    usize::from(id::session().as_bytes()[0]) % n
}
