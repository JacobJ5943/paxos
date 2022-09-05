mod acceptors;

mod proposers;


struct PromiseReturn {
    highest_ballot_num:usize,
    highest_node_identifier:usize,
    accepted_value:Option<usize>
}




#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
