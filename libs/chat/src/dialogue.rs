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

/// A Dialogue is a generic representation of a series of Posts
/// This might represent a room, group, or direct-message conversation
#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Dialogue {
    /// A title of the Dialogue
    pub title: String,
    /// A time ordered sequence of posts in the Dialogue
    posts: Vec<Post>,
    /// An index of unique Author id's (internal)
    authors: HashMap<u16, Author>,
    /// A lookup on Author names
    author_lookup: HashMap<String, u16>,
    /// The timestamp on the most recent Post
    last_timestamp: u64,
    /// The id assigned to the most recent new Author
    last_author_id: u16,
}

impl Dialogue {

    /// Creates a new Dialogue with a single Author.
    /// Author id=0 is assigned to the user of this Chat App.
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

    /// Add a new Post to the Dialogue
    ///
    /// note: posts are sorted by timestamp, so:
    /// - `post_add` at beginning or end is fast (middle triggers a binary partition)
    /// - if adding multiple posts then add oldest/newest last!
    ///
    /// # Arguments
    ///
    /// * `author` - the name of the Author of the Post
    /// * `timestamp` - the timestamp of the Post
    /// * `text` - the text content of the Post
    /// * `attach_url` - a url of an attachment (image for example)
    ///
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
                if new_ts > last_ts {
                    log::info!("insert new post at end");
                    self.posts.push(new);
                } else if new_ts < first_ts {
                    log::info!("insert new post at start");
                    self.posts.insert(0, new);
                } else {
                    log::info!("{:?}", new);
                    // insert a new post in the correct position
                    // OR replace an existing post with matching timestamp & author 
                    let i = self.posts.partition_point(|p| p.timestamp() < new_ts);
                    let last = self.posts.len() - 1;
                    for n in i..last {
                        if let Some(old) = self.posts.get(n) {
                            if old.timestamp() == new_ts {
                                if old.author_id() == author_id {
                                    log::info!("replace matching post at {n}");
                                    self.posts[i] = new;
                                    break;
                                }
                            } else {
                                log::info!("insert new post at {n}");
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

    /// Returns Some(index) of a matching Post by Author and Timestamp, or None
    ///
    /// # Arguments
    ///
    /// * `timestamp` - the Post timestamp criteria
    /// * `author` - the Post Author criteria
    ///
    pub fn post_find(&self, author: &str, timestamp: u64) -> Option<usize> {
        if let Some(author_id) = self.author_lookup.get(author) {
            let i = self.posts.partition_point(|p| p.timestamp() < timestamp);
            let last = self.posts.len() - 1;
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

    /// Return Some<Post> by index in the Dialogue, or None.
    ///
    /// # Arguments
    ///
    /// * `index` - the index of the required Post
    ///
    pub fn post_get(&self, index: usize) -> Option<&Post> {
        self.posts.get(index)
    }

    /// Return the index of the most recent Post in the Dialogue
    ///
    pub fn post_last(&self) -> Option<usize> {
        if self.posts.len() == 0 {
            None
        } else {
            Some(self.posts.len() - 1)
        }
    }

    /// Return an iterator over the Dialogue Posts (oldest first)
    ///
    pub fn posts(&self) -> Iter<Post> {
        return self.posts.iter();
    }

    /// Return Some<Author> by id, or None.
    ///
    /// # Arguments
    ///
    /// * `id` - the index of the required Author
    ///
    pub fn author(&self, id: u16) -> Option<&Author> {
        self.authors.get(&id)
    }


    /// Return Some<author_id> by Author name, or None.
    ///
    /// # Arguments
    ///
    /// * `author` - the (external) name of the Author
    ///
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

    /// Assign and Return Some<author_id>, or None
    ///
    fn author_id_next(&mut self) -> Option<u16> {
        if self.last_author_id < u16::max_value() {
            self.last_author_id += 1;
            Some(self.last_author_id)
        } else {
            None
        }
    }
}
