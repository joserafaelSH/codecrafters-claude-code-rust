use std::{collections::HashMap, fs, path};

#[derive(Debug)]
pub struct SkillsMap {
    pub map: HashMap<String, Skill>,
}

impl SkillsMap {
    pub fn get_skills_resume(&self) -> String {
        let mut skill_message = String::new();
        skill_message.push_str("skills:\n\n");

        for skill in self.map.values() {
            skill_message.push_str(&skill.get_skill_resume());
        }

        skill_message
    }

    pub fn new(path: &str) -> Self {
        let skills_map = find_skills(path);
        Self { map: skills_map }
    }
}

#[derive(Debug)]
pub struct Skill {
    pub name: String,
    description: String,
    path: String,
    pub context: String,
}

impl Skill {
    pub fn get_skill_resume(&self) -> String {
        let mut skill_message = String::new();
        skill_message.push_str(&format!(
            "- name: {}\n  description: {}.\n\n",
            self.name, self.description
        ));

        skill_message
    }

    fn get_skill_content_as_string(&self) -> Option<String> {
        match fs::read_to_string(&self.path) {
            Ok(content) => Some(content),
            Err(_) => None,
        }
    }

    pub fn get_skill_body(&self) -> Option<String> {
        if let Some(raw) = self.get_skill_content_as_string() {
            let body = match raw
                .strip_prefix("---")
                .and_then(|rest| rest.split_once("---"))
            {
                Some((_, body)) => body,
                None => &raw,
            };
            return Some(body.trim().to_string());
        }
        None
    }
}

fn get_skill_metadata(path: &str) -> Option<Skill> {
    let skill_file_path = path::Path::new(path);

    if let Ok(contents) = fs::read_to_string(skill_file_path) {
        let mut lines = contents.lines();
        if let Some(first_line) = lines.next() {
            if first_line.trim() == "---" {
                let mut name = String::new();
                let mut description = String::new();
                let mut context = String::new();
                for line in lines {
                    if line.trim() == "---" {
                        break;
                    }
                    if line.starts_with("name:") {
                        name = line["name:".len()..].trim().to_string();
                    } else if line.starts_with("description:") {
                        description = line["description:".len()..].trim().to_string();
                    } else if line.starts_with("context:") {
                        context = line["context:".len()..].trim().to_string();
                    }
                }
                return Some(Skill {
                    context,
                    description,
                    name,
                    path: path.to_string(),
                });
            }
        }
    }
    return None;
}

fn find_skills(path: &str) -> HashMap<String, Skill> {
    let mut skills_map: HashMap<String, Skill> = HashMap::new();
    let skills_path = path::Path::new(path);

    if let Ok(dir_entries) = fs::read_dir(skills_path) {
        for dir_entry in dir_entries {
            if let Ok(entry) = dir_entry {
                let path = entry.path();
                if path.is_dir() {
                    // Replace for a constant
                    let skill_file_path = path.join("SKILL.md");
                    if let Some(skill_file_path_str) = skill_file_path.to_str() {
                        if let Some(skill) = get_skill_metadata(skill_file_path_str) {
                            skills_map.insert(skill.name.clone(), skill);
                        }
                    }
                }
            } else {
                // error from dir_entry
            }
        }
    } else {
        // error from read_dir
    }

    skills_map
}
