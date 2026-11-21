use crate::domain::{Product, CURRENCY_BRL};
use crate::store::{Store, StoreError};

pub fn products() -> Vec<Product> {
    vec![
        p("bola-futebol-pro", "Bola de Futebol Profissional", "Bola oficial de 32 gomos, costura termosselada para campo. Ideal para treinos e jogos amadores.", "futebol", 19990, "⚽", 40),
        p("bola-futebol-society", "Bola de Society", "Bola de society com miolo removível e acabamento PU. Melhor controle em quadra.", "futebol", 12990, "🟢", 40),
        p("bola-futsal", "Bola de Futsal", "Baixa bounce, peso homologado para futsal indoor.", "futebol", 13990, "⚪", 25),
        p("chuteira-campo", "Chuteira Campo Trava Mista", "Cabedal sintético, travas mistas para gramado natural. Numeração unissex.", "futebol", 32990, "👟", 25),
        p("caneleira", "Caneleira (par)", "Par de caneleiras leves com tornozeleira elástica.", "futebol", 5990, "🛡️", 50),
        p("faixa-capitao", "Faixa de Capitão", "Faixa elástica ajustável com velcro. Um tamanho.", "futebol", 3990, "🎖️", 40),
        p("bola-basquete", "Bola de Basquete Indoor/Outdoor", "Bola tamanho 7, cobertura de borracha para quadra aberta ou ginásio.", "basquete", 18990, "🏀", 30),
        p("bola-volei", "Bola de Vôlei de Praia", "Mikasa-style, câmara 8 gomos, costura reforçada para areia.", "volei", 15990, "🏐", 30),
        p("rede-volei", "Rede de Vôlei 9,5 m", "Rede oficial com faixa superior de lona e cabo de aço.", "volei", 16990, "🥅", 10),
        p("raquete-tenis", "Raquete de Tênis Grafite", "Raquete 27\", peso 280 g, corda incluída. Capa de brinde.", "tenis", 45990, "🎾", 12),
        p("bola-tenis", "Pack 3 Bolas de Tênis", "Tubo pressurizado com 3 bolas para saibro ou hard court.", "tenis", 4990, "🟡", 80),
        p("raquete-beach", "Raquete de Beach Tennis", "Kevlar e carbono, formato teardrop, grip incluso.", "tenis", 38990, "🏓", 15),
        p("bola-handebol", "Bola de Handebol T2", "Bola T2 feminina/juvenil, microfibra, já lubrificada.", "handebol", 11990, "🤾", 20),
        p("luvas-boxe", "Luvas de Boxe 14 oz", "Par 14 oz com fechamento em velcro e palma ventilada.", "luta", 18990, "🥊", 20),
        p("kimono-bjj", "Kimono Jiu-Jitsu A2", "Trama pearl, calça reforçada, faixa branca inclusa.", "luta", 42990, "🥋", 10),
        p("tenis-corrida", "Tênis de Corrida Amortecimento", "Entressola EVA, drop 8 mm, indicada para asfalto até 10 km.", "corrida", 34990, "🏃", 30),
        p("oculos-natacao", "Óculos de Natação Antiembaçante", "Lente espelhada, ajuste nasal e tira de silicone.", "natacao", 7990, "🥽", 40),
        p("prancha-bodyboard", "Prancha Bodyboard 41\"", "Núcleo EPS, stringer de aluminio, leash incluso.", "agua", 24990, "🏄", 8),
        p("capacete-ciclo", "Capacete de Ciclismo In-Mold", "18 entradas de ar, regulagem occipital, certificação CE.", "ciclismo", 27990, "⛑️", 18),
        p("skate", "Skate Completo 8.0", "Shape maple canadense, trucks 139 mm, rodas 52 mm.", "skate", 29990, "🛹", 12),
        p("bola-rugby", "Bola de Rugby Oficial", "Bola tamanho 5, gripe táctil para treino e jogo.", "rugby", 14990, "🏉", 15),
        p("bola-futebol-americano", "Bola de Futebol Americano", "Couro sintético, laces destacadas, tamanho oficial.", "futebol-americano", 15990, "🏈", 10),
        p("taco-baseball", "Taco de Baseball Liga Leve", "Taco 32\", alumínio, grip emborrachado.", "baseball", 21990, "⚾", 8),
        p("squeeze", "Squeeze Térmica 1 L", "Inox 1 L, tampa com bico, mantém gelado por ~8 h.", "acessorios", 8990, "🧴", 60),
    ]
}

fn p(
    id: &str,
    name: &str,
    description: &str,
    category: &str,
    price_cents: i64,
    emoji: &str,
    stock: i32,
) -> Product {
    Product {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        category: category.into(),
        price_cents,
        currency: CURRENCY_BRL.into(),
        emoji: emoji.into(),
        available: stock,
        reserved: 0,
        sold: 0,
        reservations: Default::default(),
    }
}

pub async fn seed(store: &dyn Store) -> Result<(), StoreError> {
    if !store.list_products().await?.is_empty() {
        return Ok(());
    }
    for product in products() {
        store.put_product(product).await?;
    }
    Ok(())
}
