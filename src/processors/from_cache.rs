use crate::processors::issue_structs::IssueList;
use std::fs::File;
use std::io::BufReader;

pub fn print_cache() -> Result<IssueList, std::io::Error> {
    // get json
    //     // Open the file containing the JSON data
    let file = File::open(".git/issue_cache.json")?;
    let reader = BufReader::new(file);

    // Deserialize the JSON into a MyStruct instance
    let my_data: IssueList = serde_json::from_reader(reader)?;

    Ok(my_data)
}
