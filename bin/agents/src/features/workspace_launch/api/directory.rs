use bas::service::pb;
use gds::ui::picker::directory::{Entry, Location, Page};

pub(super) fn page(response: pb::BrowseServerDirectoriesResponse) -> Page {
    let ancestors = ancestors(&response.path);
    Page {
        location: location(response.path),
        parent: response.parent,
        ancestors,
        entries: response
            .directories
            .into_iter()
            .map(|entry| Entry {
                hidden: entry.name.starts_with('.'),
                location: Location {
                    path: entry.path,
                    label: entry.name,
                },
            })
            .collect(),
        next_cursor: response.next_cursor,
    }
}

fn location(path: String) -> Location {
    let label = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(&path)
        .to_owned();
    Location { path, label }
}

// Interpret the server's separators, never the frontend OS's Path components.
fn ancestors(path: &str) -> Vec<Location> {
    let separator = if path.starts_with('/') { '/' } else { '\\' };
    let root_end = if path.starts_with('/') {
        1
    } else if path.starts_with("\\\\") {
        path.match_indices('\\')
            .nth(3)
            .map_or(path.len(), |(index, _)| index)
    } else {
        path.find('\\').map_or(path.len(), |index| index + 1)
    };
    if path.len() <= root_end {
        return Vec::new();
    }
    let mut roots = vec![location(path[..root_end].to_owned())];
    for (index, _) in path.match_indices(separator) {
        if index > root_end && index < path.len() - 1 {
            roots.push(location(path[..index].to_owned()));
        }
    }
    roots
}
