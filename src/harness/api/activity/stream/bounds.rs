use super::super::LivePost;

pub(super) fn bound_posts(posts: &mut Vec<LivePost>) {
    const MAX_POSTS: usize = 500;
    if posts.len() > MAX_POSTS {
        posts.drain(..posts.len() - MAX_POSTS);
    }
}
