use ansi_term::Style;
use colored::Colorize;
use csscolorparser::parse;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Deserialize, Debug)]
pub struct Config {
    pub user: String,
    pub token: String,
}

#[derive(Deserialize, Debug)]
#[warn(dead_code)]
pub struct Repo {
    pub url: String,
    pub organisation: String,
    pub repo: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Assignee {
    login: String,
}

impl std::fmt::Display for Assignee {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{},", self.login)
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct AssigneeList(Vec<Assignee>);

impl AssigneeList {
    fn count_string(&self) -> usize {
        let mut counter = 0;
        for i in &self.0 {
            counter += i.login.len() + 1
        }
        counter
    }
}

impl std::fmt::Display for AssigneeList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for v in &self.0 {
            write!(f, "{}", v)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Labels {
    id: u64,
    name: String,
    color: String,
    description: String,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct LabelsList(Vec<Labels>);

impl std::fmt::Display for LabelsList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for v in &self.0 {
            match parse(v.color.as_str()) {
                Ok(color) => {
                    let [r, g, b, _a] = color.to_rgba8();
                    write!(f, "{},", v.name.truecolor(r, g, b))?;
                }
                Err(e) => {
                    eprintln!("Failed to parse color: {}", e);
                }
            }
        }
        Ok(())
    }
}

impl LabelsList {
    fn count_string(&self) -> usize {
        let mut counter = 0;
        for i in &self.0 {
            counter += i.name.len() + 1
        }
        counter
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Issue {
    number: u16,
    title: String,
    body: String,
    state: String,
    locked: bool,
    labels: LabelsList,
    assignees: AssigneeList,
    author_association: String,
    created_at: String,
    updated_at: String,

    #[serde(deserialize_with = "null_to_default")]
    closed_at: String,

    #[serde(deserialize_with = "null_to_default")]
    milestone: String,

    #[serde(deserialize_with = "null_to_default")]
    state_reason: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let issue_no = format!("{}", self.number.to_string().blue());

        write!(
            f,
            "| {}\t | {}\t | {} |\n",
            issue_no, self.title, self.assignees
        )
    }
}

// Taken from Steven Marnachs answer here: https://stackoverflow.com/questions/69225348/transforming-null-in-json-to-empty-string-instead-of-none
pub fn null_to_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let key = Option::<T>::deserialize(de)?;
    Ok(key.unwrap_or_default())
}

#[derive(Serialize, Deserialize)]
pub struct IssueList {
    pub issue_data: Vec<Issue>,
    pub meta_data: String,
}

impl IssueList {
    pub fn get_max_title(&self) -> (usize, String) {
        let longest_title = &self
            .issue_data
            .iter()
            .map(|x| x.title.clone().len())
            .max()
            .unwrap_or(0);

        let len_title = "-".repeat(longest_title.to_owned() + 4);

        (longest_title.to_owned(), len_title)
    }

    pub fn get_max_number(&self) -> (usize, String) {
        let widest_number = self
            .issue_data
            .iter()
            .map(|x| x.number.to_string().clone().len())
            .max()
            .unwrap_or(0);

        // Why 5 for a Tab for this data?
        let len_number = "-".repeat(widest_number + 5);

        (widest_number, len_number)
    }

    pub fn get_max_labels(&self) -> (usize, String) {
        let mut collection = Vec::new();
        for c in &self.issue_data {
            collection.push(c.labels.count_string());
        }

        // Dont see why this has to be done, when it isn;t required for the previous get_max... functions
        let usized_0 = usize::try_from(0).unwrap();
        let widest_labels = collection.iter().max().unwrap_or(&usized_0);

        let len_labels = "-".repeat(widest_labels.to_owned());

        (widest_labels.to_owned(), len_labels)
    }

    pub fn get_max_assignee(&self) -> (usize, String) {
        let mut collection = Vec::new();
        for c in &self.issue_data {
            collection.push(c.assignees.count_string());
        }

        // Dont see why this has to be done, when it isn;t required for the previous get_max... functions
        let usized_0 = usize::try_from(0).unwrap();
        let widest_assignee = collection.iter().max().unwrap_or(&usized_0);
        let len_assignee = "-".repeat(widest_assignee.to_owned());

        (widest_assignee.to_owned(), len_assignee)
    }

    pub fn trim_titles(&mut self, terminal_length: &usize) -> &mut IssueList {
        if terminal_length.to_owned() != 0 {
            for c in &mut self.issue_data {
                if c.title.len() > terminal_length.to_owned() {
                    c.title = format!("{}...", &c.title[..terminal_length.to_owned()])
                }
            }
        }

        self
    }
}

impl std::fmt::Display for IssueList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}GitHub Issues for {}\n",
            "|".truecolor(105, 105, 105),
            Style::new()
                .underline()
                .bold()
                .paint(self.meta_data.to_owned())
        )?;

        let (longest_title, _len_title) = &self.get_max_title();
        let (longest_number, _len_number) = &self.get_max_number();
        let (longest_labels, _len_labels) = &self.get_max_labels();
        let (longest_assignee, _len_assignee) = &self.get_max_assignee();

        // causes 'attempt to subtract with overflow' when only 1 issue and issue number = 1 This is because "NO." has len longer than "1"
        // Need fix
        let issue_no = if longest_number <= &"NO.".to_string().len() {
            0
        } else {
            longest_number - "NO.".to_string().len()
        };

        let issue_spaces = " ".repeat(issue_no);

        let title_no = longest_title - "ISSUE TITLE".to_string().len();
        let title_spaces = " ".repeat(title_no);

        let labels_no = longest_labels - "LABELS".to_string().len();
        let labels_spaces = " ".repeat(labels_no + 1);

        let assignee_no = longest_assignee - "ASSIGNEE".to_string().len();

        // Normally this adds an extra 1, I'm not sure why. Because I changed the header from ASSIGNEE to ASSIGNEE/S we need to remove 2 spaces. Hence, -1.
        let assignee_spaces = " ".repeat(assignee_no - 1);

        // borked
        write!(
            f,
            "{}{}{}{}{}{}{}{}{}{}{}{}{}\n",
            "|".truecolor(105, 105, 105),
            Style::new().underline().paint("NO."),
            Style::new().underline().paint(issue_spaces),
            "|".truecolor(105, 105, 105),
            Style::new().underline().paint("ISSUE TITLE"),
            Style::new().underline().paint(title_spaces),
            "|".truecolor(105, 105, 105),
            Style::new().underline().paint("LABELS"),
            Style::new().underline().paint(labels_spaces),
            "|".truecolor(105, 105, 105),
            Style::new().underline().paint("ASSIGNEE/S"),
            Style::new().underline().paint(assignee_spaces),
            "|".truecolor(105, 105, 105),
        )?;
        for v in &self.issue_data {
            let len_number_remainder = if v.number.to_string().len() < "NO.".to_string().len() {
                let len_number = "NO.".to_string().len() - v.number.to_string().len();
                len_number
            } else {
                let len_inner_number = v.number.to_string().len();
                let output = longest_number - &len_inner_number;
                output
            };
            let number_space = " ".repeat(len_number_remainder);

            let len_inner_title = v.title.len();
            let len_title_remainder = longest_title - &len_inner_title;
            let title_space = " ".repeat(len_title_remainder);

            let len_inner_label = v.labels.count_string();
            let len_labels_remainder = longest_labels - len_inner_label;
            let label_space = " ".repeat(len_labels_remainder + 1);

            let len_inner_assignee = v.assignees.count_string();
            let len_assignee_remainder = longest_assignee - len_inner_assignee;
            let assignee_space = " ".repeat(len_assignee_remainder + 1);
            write!(
                f,
                "{}{}{}{}{}{}{}{}{}{}{}{}{}\n",
                "|".truecolor(105, 105, 105),
                v.number.to_string().blue(),
                number_space,
                "|".truecolor(105, 105, 105),
                v.title,
                title_space,
                "|".truecolor(105, 105, 105),
                v.labels,
                label_space,
                "|".truecolor(105, 105, 105),
                v.assignees,
                assignee_space,
                "|".truecolor(105, 105, 105),
            )?;
        }
        Ok(())
    }
}
