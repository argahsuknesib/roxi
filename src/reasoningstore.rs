extern crate oxigraph;

pub mod triple;
pub mod ruleindex;
pub mod rule;
pub mod binding;
use std::collections::HashMap;
use oxigraph::MemoryStore;
use oxigraph::model::*;
use oxigraph::sparql::{QueryResults, Query};
use oxigraph::io::{DatasetFormat};
use oxigraph::model::NamedOrBlankNode;
use rio_turtle::{TurtleParser, TurtleError};
use rio_api::parser::TriplesParser;
use std::io::BufRead;
use std::io;
use std::rc::Rc;

use triple::ReasonerTriple;
use crate::reasoningstore::rule::Rule;
use crate::reasoningstore::ruleindex::RuleIndex;
use crate::n3_parser::parse;
use crate::reasoningstore::binding::Binding;


pub struct ReasoningStore {
    pub store: MemoryStore,
    pub(crate) reasoning_store: MemoryStore,
    rules: Vec<Rc<Rule>>,
    rules_index: RuleIndex,
}



impl ReasoningStore {
    fn eval_triple_element(&self, left: &NamedOrBlankNode, right:&NamedOrBlankNode) -> bool{
        if left.is_blank_node() && right.is_blank_node(){
            true
        }else{
            left.eq(right)
        }
    }
    pub(crate) fn find_subrules(&self, rule_head: &ReasonerTriple) -> Vec<Rc<Rule>> {
        let mut rule_matches = Vec::new();
        for rule in self.rules.iter(){
            let head:&ReasonerTriple = &rule.head;
            if self.eval_triple_element(&head.s,&rule_head.s) &&
                self.eval_triple_element(&head.p,&rule_head.p) &&
                self.eval_triple_element(&head.o,&rule_head.o) {
                rule_matches.push(rule.clone());
            }
        }
        rule_matches
    }
    pub(crate) fn eval_rule_atom(&self, rule_atom: &ReasonerTriple) -> Binding{
        let query_string = format!("Select *  WHERE {{ {} }}", rule_atom.to_string());
        let mut bindings = Binding::new();
        if let QueryResults::Solutions(mut solutions) = self.store.query(&query_string).unwrap() {
            for sol in solutions.into_iter() {
                match sol {
                    Ok(s) => s.iter().for_each(|b| bindings.add(b.0.as_str(),b.1.clone())),
                    Err(_) => print!("error"),
                }
            }
        }
        bindings
    }
}


pub fn convert_to_query(rule: &Rule) -> Query {
    let body_string: String = rule.body.iter().map(|r| r.to_string()).collect();
    let query_string = format!("CONSTRUCT {{ {} }} WHERE {{ {} }}", rule.head.to_string(), body_string);
    Query::parse(&query_string, None).unwrap()
}


impl ReasoningStore {
    pub fn new() -> ReasoningStore {
        ReasoningStore {
            store: MemoryStore::new(),
            reasoning_store: MemoryStore::new(),
            rules: Vec::new(),
            rules_index: RuleIndex::new(),
        }
    }
    pub fn load_abox(&self, reader: impl BufRead) -> Result<(), io::Error> {
        self.store.load_dataset(reader, DatasetFormat::TriG, None)
    }
    pub fn load_tbox(&mut self, reader: impl BufRead) {
        let rdf_subclass = String::from("<http://www.w3.org/2000/01/rdf-schema#subClassOf>");
        TurtleParser::new(reader, None).parse_all(&mut |triple| {
            if triple.predicate.to_string().eq(&rdf_subclass) {
                let str_len = triple.object.to_string().len();
                let object_str = &triple.object.to_string()[1..str_len - 1];
                let str_len = triple.subject.to_string().len();
                let subject_str = &triple.subject.to_string()[1..str_len - 1];
                if let Ok(named_subject) = NamedNode::new(subject_str) {
                    let body = ReasonerTriple { s: NamedOrBlankNode::from(BlankNode::new("s").unwrap()), p: NamedOrBlankNode::from(NamedNode::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap()), o: NamedOrBlankNode::from(named_subject) };
                    if let Ok(named) = NamedNode::new(object_str) {
                        let head = ReasonerTriple { s: NamedOrBlankNode::from(BlankNode::new("s").unwrap()), p: NamedOrBlankNode::from(NamedNode::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap()), o: NamedOrBlankNode::from(named) };
                        let mut body_rules = Vec::new();
                        body_rules.push(body);
                        let rule = Rc::new(Rule { body: body_rules, head });
                        self.add_rule(rule.clone());
                    }
                }
            }
            Ok(()) as Result<(), TurtleError>
        }).unwrap();
    }
    pub fn parse_and_add_rule(&mut self, parse_string: &str){
        let rules_results = parse(parse_string);
        if let Ok(rules) = rules_results{
            rules.into_iter().for_each(|r| self.add_rule(Rc::new(r)));
        }
    }
    pub fn len_rules(&self) -> usize {
        self.rules.len()
    }
    pub fn len_abox(&self) -> usize {
        self.store.len()
    }
    pub fn add_rule(&mut self, rule: Rc<Rule>) {
        self.rules.push(rule.clone());
        self.rules_index.add(rule.clone());
    }
    pub fn materialize(&mut self) {
        self.reasoning_store.clear();
        let mut tripe_queue: Vec<Quad> = self.store.iter().collect();
        // iterate over all triples
        let mut queue_iter = 0;
        while queue_iter < tripe_queue.len() {
            let mut temp_triples = Vec::new();
            let quad = tripe_queue.get(queue_iter).unwrap();
            if !self.reasoning_store.contains(quad) {
                self.reasoning_store.insert(quad.clone());
                self.store.insert(quad.clone());
                //let matched_rules = find_rule_match(&quad, &rules); // without indexing
                let matched_rules = self.rules_index.find_match(&quad);
                // find matching rules
                for matched_rule in matched_rules.into_iter() {
                    let q = convert_to_query(matched_rule);
                    if let QueryResults::Graph(solutions) = self.reasoning_store.query(q).unwrap() {
                        for sol in solutions.into_iter() {
                            match sol {
                                Ok(s) => temp_triples.push(Quad::new(s.subject.clone(), s.predicate.clone(), s.object.clone(), None)),
                                _ => (),
                            }
                        }
                    }
                }
            }
            queue_iter += 1;
            temp_triples.iter().for_each(|t| tripe_queue.push(t.clone()));
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_abox() {
        let store = ReasoningStore::new();
        store.load_abox( b"<http://example2.com/a> <http://rdf/type> <http://test.com/C1> .".as_ref());
        assert_eq!(store.len_abox(), 1);
    }

    #[test]
    fn test_add_rule() {
        let mut store = ReasoningStore::new();
        store.parse_and_add_rule("@prefix test: <http://www.test.be/test#>.\n{?VaRr0 <http://test.be/pieter> ?lastVar. ?VaRr0 test:type ?lastVar.}=>{?VaRr0 test:hasValue ?lastVar.}");
        assert_eq!(store.len_rules(), 1);
    }
    #[test]
    fn test_single_rule() {
        let mut store = ReasoningStore::new();
        store.parse_and_add_rule("@prefix test: <http://www.test.be/test#>.\n @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>.\n {?s rdf:type test:SubClass. }=>{?s rdf:type test:SuperType.}");
        store.load_abox( b"<http://example2.com/a> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.test.be/test#SubClass> .".as_ref());
        assert_eq!(store.len_abox(), 1);
        store.materialize();
        assert_eq!(store.len_abox(), 2);
        let quad_ref = QuadRef::new(NamedNodeRef::new("http://example2.com/a").unwrap(),
                                            NamedNodeRef::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap(),
                                            NamedNodeRef::new("http://www.test.be/test#SuperType").unwrap(), None);
        assert!(store.store.contains(quad_ref));
    }

    #[test]
    fn test_join_rule() {
        let mut store = ReasoningStore::new();
        store.parse_and_add_rule("@prefix test: <http://www.test.be/test#>.\n @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>.\n {?s rdf:type test:SubClass. ?s test:hasRef ?o.}=>{?s rdf:type test:SuperType.}");
        store.load_abox( b"<http://example2.com/a> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.test.be/test#SubClass> .".as_ref());
        store.load_abox( b"<http://example2.com/a> <http://www.test.be/test#hasRef> <http://example2.com/b> .".as_ref());

        assert_eq!(store.len_abox(), 2);
        store.materialize();
        assert_eq!(store.len_abox(), 3);
        let quad_ref = QuadRef::new(NamedNodeRef::new("http://example2.com/a").unwrap(),
                                    NamedNodeRef::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap(),
                                    NamedNodeRef::new("http://www.test.be/test#SuperType").unwrap(), None);
        assert!(store.store.contains(quad_ref));
    }
    #[test]
    fn test_long_join_rule() {
        let mut store = ReasoningStore::new();
        store.parse_and_add_rule("@prefix test: <http://www.test.be/test#>.\n @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>.\n {?s rdf:type test:SubClass. ?s test:hasRef ?o. ?o test:hasRef ?o2. ?o2 rdf:type test:SubClass.}=>{?s rdf:type test:SuperType.}");
        store.load_abox( b"<http://example2.com/a> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.test.be/test#SubClass> .".as_ref());
        store.load_abox( b"<http://example2.com/b> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.test.be/test#SubClass> .".as_ref());
        store.load_abox( b"<http://example2.com/c> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.test.be/test#SubClass> .".as_ref());
        store.load_abox( b"<http://example2.com/a> <http://www.test.be/test#hasRef> <http://example2.com/b> .".as_ref());
        store.load_abox( b"<http://example2.com/b> <http://www.test.be/test#hasRef> <http://example2.com/c> .".as_ref());

        assert_eq!(store.len_abox(), 5);
        store.materialize();
        assert_eq!(store.len_abox(), 6);
        let quad_ref = QuadRef::new(NamedNodeRef::new("http://example2.com/a").unwrap(),
                                    NamedNodeRef::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").unwrap(),
                                    NamedNodeRef::new("http://www.test.be/test#SuperType").unwrap(), None);
        assert!(store.store.contains(quad_ref));
    }

    #[test]
    fn test_find_backward_rules(){
        let mut store = ReasoningStore::new();
        store.parse_and_add_rule("@prefix test: <http://www.test.be/test#>.\n @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>.\n \
        {?s rdf:type test:SubClass. ?s test:hasRef ?o. ?o test:hasRef ?o2. ?o2 rdf:type test:SubClass.}=>{?s rdf:type test:SuperType.}");
        //match
        let backward_head = ReasonerTriple::new("?s".to_string(),"http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),"http://www.test.be/test#SuperType".to_string());
        let sub_rules : Vec<Rc<Rule>> = store.find_subrules(&backward_head);
        assert_eq!(sub_rules.len(), 1);
        // no match
        let backward_head = ReasonerTriple::new("?s".to_string(),"http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),"http://www.test.be/test#SuperTypeNotFound".to_string());
        let sub_rules : Vec<Rc<Rule>> = store.find_subrules(&backward_head);
        assert_eq!(sub_rules.len(), 0);
    }
    #[test]
    fn test_find_backward_rules_with_diff_var_names(){
        let mut store = ReasoningStore::new();
        store.parse_and_add_rule("@prefix test: <http://www.test.be/test#>.\n @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>.\n \
        {?s rdf:type test:SubClass. ?s test:hasRef ?o. ?o test:hasRef ?o2. ?o2 rdf:type test:SubClass.}=>{?s rdf:type test:SuperType.}");
        // diff variable names
        let backward_head = ReasonerTriple::new("?newVar".to_string(),"http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),"http://www.test.be/test#SuperType".to_string());
        let sub_rules : Vec<Rc<Rule>> = store.find_subrules(&backward_head);
        assert_eq!(sub_rules.len(), 1);
    }



    #[test]
    fn test_eval_rule_atom(){
        let mut store = ReasoningStore::new();
        store.load_abox( b"<http://example2.com/a> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.test.be/test#SubClass> .".as_ref());

        let rule_atom = ReasonerTriple::new("?s".to_string(),"http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),"http://www.test.be/test#SubClass".to_string());
        let result_bindings = store.eval_rule_atom(&rule_atom);
        let mut binding: Binding = Binding::new();
        binding.add("s", Term::from(NamedNode::new("http://example2.com/a".to_string()).unwrap()));
        assert_eq!(binding, result_bindings);

    }



}
