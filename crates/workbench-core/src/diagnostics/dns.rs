//! One transmission per name/type to one configured endpoint, with bounded CNAME association.
//! The high-level resolver is intentionally excluded: its cache can synthesize special names,
//! its retry handle resends requests, and its name-server layer reconnects closed TCP streams.
use super::config;
use hickory_resolver::{
    net::{
        runtime::TokioRuntimeProvider,
        tcp::TcpClientStream,
        udp::UdpClientStream,
        xfer::{DnsExchange, DnsHandle, FirstAnswer},
    },
    proto::{
        op::{DnsRequestOptions, DnsResponse, MessageType, OpCode, Query, ResponseCode},
        rr::{DNSClass, Name, RData, Record, RecordType},
    },
};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

pub(super) const MAX_CNAME_HOPS: usize = 8;

pub(super) struct Family {
    pub addresses: Vec<IpAddr>,
    pub chain: Vec<String>,
}

pub(super) async fn exchange(
    peer: SocketAddr,
    transport: &str,
    timeout: Duration,
) -> Result<(DnsExchange<TokioRuntimeProvider>, TokioRuntimeProvider), &'static str> {
    let provider = TokioRuntimeProvider::default();
    let exchange = if transport == "tcp" {
        // One connected stream for both families and all aliases. A closed stream is terminal.
        TcpClientStream::exchange(peer, None, timeout, Some(2), provider.clone())
            .await
            .map_err(|_| "dns_resolver_unavailable")?
    } else {
        UdpClientStream::builder(peer, provider.clone())
            .with_timeout(Some(timeout))
            .with_max_retries(0)
            .exchange()
    };
    // The caller owns the provider's JoinSet for the whole exchange. In particular, TCP's
    // stream does not retain it, so dropping it here would abort I/O before the first query.
    Ok((exchange, provider))
}

fn valid_response(response: &DnsResponse, query: &Query) -> Result<(), &'static str> {
    if response.metadata.message_type != MessageType::Response
        || response.metadata.op_code != OpCode::Query
        || response.queries.as_slice() != [query.clone()]
    {
        return Err("dns_response_invalid");
    }
    if response.metadata.truncation {
        return Err("dns_response_truncated");
    }
    if response.metadata.response_code != ResponseCode::NoError {
        return Err("dns_lookup_failed");
    }
    Ok(())
}

// Some recursive servers include the whole CNAME chain in one answer; others return only an
// alias. None means the new terminal name needs its own single query to this same endpoint.
fn associated(
    answers: &[Record],
    queried: &Name,
    chain: &mut Vec<Name>,
    kind: RecordType,
) -> Result<Option<Vec<IpAddr>>, &'static str> {
    loop {
        let current = chain.last().expect("original name exists");
        let mut alias = None;
        let mut addresses = Vec::new();
        for record in answers
            .iter()
            .filter(|record| record.name == *current && record.dns_class == DNSClass::IN)
        {
            if let RData::CNAME(target) = &record.data {
                if alias.as_ref().is_some_and(|name| name != &target.0) {
                    return Err("dns_cname_conflict");
                }
                alias = Some(target.0.clone());
            } else if let Some(address) = record.data.ip_addr() {
                if record.record_type() != kind {
                    return Err("dns_answer_type_mismatch");
                }
                addresses.push(address);
                if addresses.len() > 16 {
                    return Err("dns_answer_limit_exceeded");
                }
            }
        }
        let Some(alias) = alias else {
            return Ok((!addresses.is_empty() || current == queried).then_some(addresses));
        };
        if !addresses.is_empty() {
            return Err("dns_cname_conflict");
        }
        if chain.contains(&alias) {
            return Err("dns_cname_cycle");
        }
        if chain.len() > MAX_CNAME_HOPS {
            return Err("dns_cname_limit_exceeded");
        }
        if !config::hostname(alias.to_ascii().trim_end_matches('.')) {
            return Err("dns_alias_invalid");
        }
        chain.push(alias);
    }
}

pub(super) async fn family(
    exchange: &DnsExchange<TokioRuntimeProvider>,
    hostname: &str,
    kind: RecordType,
    udp: bool,
    questions: &mut Vec<Query>,
) -> Result<Family, &'static str> {
    let name = Name::from_ascii(format!("{hostname}.")).map_err(|_| "dns_response_invalid")?;
    let mut chain = vec![name];
    let mut options = DnsRequestOptions::default();
    options.case_randomization = udp;
    loop {
        let name = chain.last().expect("original name exists").clone();
        let query = Query::query(name.clone(), kind);
        questions.push(query.clone());
        let response = exchange
            .lookup(query.clone(), options)
            .first_answer()
            .await
            .map_err(|_| "dns_lookup_failed")?;
        valid_response(&response, &query)?;
        if let Some(addresses) = associated(&response.answers, &name, &mut chain, kind)? {
            return Ok(Family {
                addresses,
                chain: chain
                    .iter()
                    .map(|name| name.to_ascii().trim_end_matches('.').to_ascii_lowercase())
                    .collect(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_resolver::proto::{
        op::Message,
        rr::rdata::{A, CNAME},
    };
    use std::net::Ipv4Addr;

    fn name(value: &str) -> Name {
        Name::from_ascii(value).unwrap()
    }
    fn a(owner: &str, last: u8) -> Record {
        Record::from_rdata(name(owner), 0, RData::A(A(Ipv4Addr::new(127, 0, 0, last))))
    }
    fn alias(owner: &str, target: &str) -> Record {
        Record::from_rdata(name(owner), 0, RData::CNAME(CNAME(name(target))))
    }
    fn collect(answers: &[Record]) -> Result<(Option<Vec<IpAddr>>, Vec<Name>), &'static str> {
        let original = name("service.example.");
        let mut chain = vec![original.clone()];
        let result = associated(answers, &original, &mut chain, RecordType::A)?;
        Ok((result, chain))
    }

    #[test]
    fn ignores_unrelated_owners_and_other_classes() {
        let mut other_class = a("service.example.", 3);
        other_class.dns_class = DNSClass::CH;
        let (addresses, chain) = collect(&[
            a("other.example.", 1),
            a("SERVICE.EXAMPLE.", 2),
            other_class,
        ])
        .unwrap();
        assert_eq!(
            addresses.unwrap(),
            [IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))]
        );
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn follows_only_established_cname_chain_in_any_record_order() {
        let (addresses, chain) = collect(&[
            a("last.example.", 2),
            alias("next.example.", "last.example."),
            a("unrelated.example.", 1),
            alias("service.example.", "next.example."),
        ])
        .unwrap();
        assert_eq!(
            addresses.unwrap(),
            [IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))]
        );
        assert_eq!(
            chain,
            [
                name("service.example."),
                name("next.example."),
                name("last.example.")
            ]
        );
    }

    #[test]
    fn alias_without_terminal_answer_requires_query_not_synthesis() {
        let (addresses, chain) =
            collect(&[alias("service.example.", "127.in-addr.arpa.")]).unwrap();
        assert!(addresses.is_none());
        assert_eq!(chain, [name("service.example."), name("127.in-addr.arpa.")]);
        let queried = name("127.in-addr.arpa.");
        let mut chain = chain;
        assert_eq!(
            associated(&[], &queried, &mut chain, RecordType::A).unwrap(),
            Some(vec![])
        );
    }

    #[test]
    fn rejects_cname_cycles_conflicts_and_unsupported_aliases() {
        assert_eq!(
            collect(&[
                alias("service.example.", "next.example."),
                alias("next.example.", "SERVICE.EXAMPLE.")
            ])
            .unwrap_err(),
            "dns_cname_cycle"
        );
        assert_eq!(
            collect(&[
                alias("service.example.", "one.example."),
                alias("service.example.", "two.example.")
            ])
            .unwrap_err(),
            "dns_cname_conflict"
        );
        assert_eq!(
            collect(&[
                alias("service.example.", "one.example."),
                a("service.example.", 1)
            ])
            .unwrap_err(),
            "dns_cname_conflict"
        );
        assert_eq!(
            collect(&[alias("service.example.", "localhost.")]).unwrap_err(),
            "dns_alias_invalid"
        );
    }

    #[test]
    fn eight_hops_allowed_ninth_refused() {
        let mut answers = vec![alias("service.example.", "hop1.example.")];
        for index in 1..8 {
            answers.push(alias(
                &format!("hop{index}.example."),
                &format!("hop{}.example.", index + 1),
            ));
        }
        answers.push(a("hop8.example.", 1));
        assert_eq!(collect(&answers).unwrap().1.len(), 9);
        answers.pop();
        answers.push(alias("hop8.example.", "hop9.example."));
        assert_eq!(collect(&answers).unwrap_err(), "dns_cname_limit_exceeded");
    }

    #[test]
    fn mismatched_service_address_type_refused() {
        assert_eq!(
            associated(
                &[a("service.example.", 1)],
                &name("service.example."),
                &mut vec![name("service.example.")],
                RecordType::AAAA
            )
            .unwrap_err(),
            "dns_answer_type_mismatch"
        );
    }

    #[test]
    fn seventeen_service_addresses_refused() {
        let answers = (1..=17)
            .map(|index| a("service.example.", index))
            .collect::<Vec<_>>();
        assert_eq!(collect(&answers).unwrap_err(), "dns_answer_limit_exceeded");
    }

    #[test]
    fn validates_response_role_question_opcode_rcode_and_truncation() {
        let query = Query::query(name("service.example."), RecordType::A);
        let mut message = Message::response(7, OpCode::Query);
        message.queries.push(query.clone());
        let response = DnsResponse::from_message(message.clone()).unwrap();
        assert!(valid_response(&response, &query).is_ok());
        message.metadata.truncation = true;
        assert_eq!(
            valid_response(&DnsResponse::from_message(message.clone()).unwrap(), &query),
            Err("dns_response_truncated")
        );
        message.metadata.truncation = false;
        message.metadata.response_code = ResponseCode::NXDomain;
        assert_eq!(
            valid_response(&DnsResponse::from_message(message.clone()).unwrap(), &query),
            Err("dns_lookup_failed")
        );
        message.metadata.response_code = ResponseCode::NoError;
        message.queries[0] = Query::query(name("other.example."), RecordType::A);
        assert_eq!(
            valid_response(&DnsResponse::from_message(message.clone()).unwrap(), &query),
            Err("dns_response_invalid")
        );
        message.queries = vec![query.clone(), query.clone()];
        assert_eq!(
            valid_response(&DnsResponse::from_message(message.clone()).unwrap(), &query),
            Err("dns_response_invalid")
        );
        message.queries = vec![query.clone()];
        message.metadata.message_type = MessageType::Query;
        // Hickory's response constructor refuses this role before our response admission.
        assert!(DnsResponse::from_message(message.clone()).is_err());
        message.metadata.message_type = MessageType::Response;
        message.metadata.op_code = OpCode::Update;
        assert_eq!(
            valid_response(&DnsResponse::from_message(message).unwrap(), &query),
            Err("dns_response_invalid")
        );
    }
}
