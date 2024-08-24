use config_file::FromConfigFile;
use serde::Deserialize;

pub fn issues(config_file: &String, repo: &String, repo_override: &String) -> () {
    #[derive(Deserialize, Debug)]
    struct Config {
        user: String,
        host: String,
        pass: String,
        token: String,
    }

    let config = Config::from_config_file(config_file).unwrap();

    if repo_override.to_owned() == "-NA-".to_string() {
        let repo_config = Config::from_config_file(config_file).unwrap();
        println!("{:?}", repo_override)
    } else {
        println!("NOT DONE ANYTHING YET")
    }

    println!("{:?}", config)
}
