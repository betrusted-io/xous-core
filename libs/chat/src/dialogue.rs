pub mod attach;
pub mod author;
pub mod post;

use crate::now;

use author::Author;
use core::slice::Iter;
use post::Post;
//use crate::dialogue::{attach, author, post};
use rkyv::{Archive, Deserialize, Serialize};
use std::collections::HashMap;

use std::io::{Error, ErrorKind};

pub const MAX_BYTES: usize = 4000;

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Dialogue {
    pub title: String,
    posts: Vec<Post>,
    authors: HashMap<u16, Author>,
    author_lookup: HashMap<String, u16>,
    last_timestamp: u64,
    last_author_id: u16,
}

impl Dialogue {
    pub fn new(title: &str) -> Self {
        let first_author_id = 0;
        let author = Author::new("me");
        let mut authors = HashMap::new();
        authors.insert(first_author_id, author);
        Self {
            title: title.to_string(),
            posts: Vec::<Post>::new(),
            authors: authors,
            author_lookup: HashMap::<String, u16>::new(),
            last_timestamp: now(),
            last_author_id: first_author_id + 1,
        }
    }

    // add a new post to the dialog
    // note: posts are sorted by timestamp, so:
    // - add post at beginning or end is fast (middle triggers a binary partition)
    // - if adding multiple posts then add oldest/newest last!
    pub fn post_add(
        &mut self,
        author: &str,
        timestamp: u64,
        text: &str,
        _attach_url: Option<&str>,
    ) -> Result<(), Error> {
        match self.author_id(author) {
            Some(author_id) => {
                let new = Post::new(
                    author_id, timestamp, text, None, // TODO implement
                );
                if self.posts.len() == 0 {
                    self.posts.push(new);
                    return Ok(());
                }
                let new_ts = new.timestamp();
                let first_ts = self.posts.first().map_or(0, |p| p.timestamp());
                let last_ts = self.posts.last().map_or(0, |p| p.timestamp());
                if new_ts >= last_ts {
                    self.posts.push(new);
                } else if new_ts < first_ts {
                    self.posts.insert(0, new);
                } else {
                    let i = self.posts.partition_point(|p| p.timestamp() < new_ts);
                    let last = self.posts.len()-1;
                    for n in i..last {
                        if let Some(old) = self.posts.get(n) {
                            if old.timestamp() == new_ts {
                                if old.author_id() == author_id {
                                    // replace matching post
                                    self.posts[i] = new;
                                    break;
                                } 
                            } else {
                                // insert new post
                                self.posts.insert(n, new);
                                break;
                            }
                        }
                    }
                }
                Ok(())
            }
            None => Err(Error::new(ErrorKind::Other, "max authors exceeeded")),
        }
    }

    pub fn post_find(&self, author: &str, timestamp: u64) -> Option<usize> {
        if let Some(author_id) = self.author_lookup.get(author) {
            let i = self.posts.partition_point(|p| p.timestamp() < timestamp);
            let last = self.posts.len()-1;
            for n in i..last {
                if let Some(post) = self.posts.get(n) {
                    if post.timestamp() == timestamp {
                        if post.author_id() == *author_id {
                            return Some(n);
                        } 
                    } else {
                        break;
                    }
                }
            }
        }
        None
    }

    pub fn posts(&self) -> Iter<Post> {
        return self.posts.iter();
    }

    pub fn author(&self, id: u16) -> Option<&Author> {
        self.authors.get(&id)
    }

    // get internal author_id for external author str
    pub fn author_id(&mut self, author: &str) -> Option<u16> {
        match self.author_lookup.get(author) {
            Some(id) => Some(*id),
            None => {
                if let Some(id) = self.author_id_next() {
                    self.authors.insert(id, Author::new(author));
                    self.author_lookup.insert(author.to_string(), id);
                    Some(id)
                } else {
                    None
                }
            }
        }
    }

    // assign the next available interanl author_id
    fn author_id_next(&mut self) -> Option<u16> {
        if self.last_author_id < u16::max_value() {
            self.last_author_id += 1;
            Some(self.last_author_id)
        } else {
            None
        }
    }
}
