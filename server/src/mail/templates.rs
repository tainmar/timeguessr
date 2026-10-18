//! Email bodies (HTML + plain text). Every user-provided value goes through `escape_html`.

use crate::util::escape_html as esc;

pub struct Rendered {
    pub subject: String,
    pub html: String,
    pub text: String,
}

fn layout(preheader: &str, body_html: &str, footer: &str) -> String {
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"></head>
<body style="margin:0;padding:0;background:#f4efe6;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#1f2933;">
<span style="display:none;max-height:0;overflow:hidden;opacity:0">{preheader}</span>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f4efe6;padding:24px 12px;">
<tr><td align="center">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:560px;background:#ffffff;border-radius:14px;overflow:hidden;box-shadow:0 1px 3px rgba(0,0,0,.08);">
<tr><td style="background:#1d2a3a;padding:18px 28px;color:#f8c96b;font-size:20px;font-weight:700;letter-spacing:.3px;">&#128248; TimeGuessr</td></tr>
<tr><td style="padding:28px;font-size:16px;line-height:1.55;">{body_html}</td></tr>
<tr><td style="padding:16px 28px 24px;font-size:12px;line-height:1.5;color:#6b7785;border-top:1px solid #eee;">{footer}</td></tr>
</table></td></tr></table></body></html>"#,
        preheader = esc(preheader),
    )
}

fn button(url: &str, label: &str) -> String {
    format!(
        r#"<p style="margin:26px 0;"><a href="{url}" style="display:inline-block;background:#e8a33d;color:#1d2a3a;text-decoration:none;font-weight:700;padding:13px 24px;border-radius:10px;">{label}</a></p>
<p style="font-size:12px;color:#6b7785;word-break:break-all;">Or copy this link: {url}</p>"#,
        url = esc(url),
        label = esc(label),
    )
}

fn quote(message: &str) -> String {
    format!(
        r#"<blockquote style="margin:18px 0;padding:12px 16px;background:#fbf6ec;border-left:4px solid #e8a33d;border-radius:6px;white-space:pre-line;">{}</blockquote>"#,
        esc(message)
    )
}

fn plural(n: i64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// "3 of 12 participants haven't finished uploading yet."
pub fn pending_sentence(pending: i64, total: i64) -> String {
    if pending == 0 {
        format!("All {total} participants have finished uploading.")
    } else if pending == total {
        format!(
            "{pending} of {total} participants haven't finished uploading yet (nobody has finished so far)."
        )
    } else {
        format!(
            "{pending} of {total} participants {} finished uploading yet.",
            if pending == 1 { "hasn't" } else { "haven't" }
        )
    }
}

pub fn login(url: &str) -> Rendered {
    let body = format!(
        "<p>Hi!</p><p>Click the button below to sign in to TimeGuessr. The link expires in 30 minutes and can only be used once.</p>{}<p>If you didn't ask for this email, you can safely ignore it.</p>",
        button(url, "Sign in")
    );
    Rendered {
        subject: "Your TimeGuessr sign-in link".into(),
        html: layout("Your sign-in link", &body, "You received this email because someone entered your address on TimeGuessr."),
        text: format!("Sign in to TimeGuessr (link valid 30 minutes, single use):\n{url}\n\nIf you didn't ask for this email, ignore it."),
    }
}

pub struct CollectionEmail<'a> {
    pub game_name: &'a str,
    pub organizer: &'a str,
    pub participant_name: Option<&'a str>,
    pub message: Option<&'a str>,
    pub photos_required: i32,
    pub photos_uploaded: i64,
    pub deadline: Option<&'a str>,
    pub pending: i64,
    pub total: i64,
    pub url: &'a str,
}

fn greeting(name: Option<&str>) -> String {
    match name {
        Some(n) if !n.is_empty() => format!("Hi {},", n),
        _ => "Hi there,".into(),
    }
}

fn footer_for(organizer: &str, game: &str) -> String {
    format!(
        "You received this email because {} added you to the TimeGuessr game “{}”. This link is personal: please don't forward it.",
        esc(organizer),
        esc(game)
    )
}

pub fn invitation(e: &CollectionEmail) -> Rendered {
    let photos = plural(e.photos_required as i64, "photo", "photos");
    let deadline_html = e
        .deadline
        .map(|d| format!(" before <strong>{}</strong>", esc(d)))
        .unwrap_or_default();
    let deadline_text = e.deadline.map(|d| format!(" before {d}")).unwrap_or_default();
    let stats = pending_sentence(e.pending, e.total);
    let body = format!(
        r#"<p>{greet}</p>
<p><strong>{org}</strong> is preparing a TimeGuessr game: <strong>{game}</strong>. Everyone shares photos of themselves, then teams try to guess <em>when</em> and <em>where</em> each one was taken.</p>
{message}
<p>Please share <strong>{photos} of yourself</strong>, with the year and the exact place it was taken{deadline}. Old photos make the best puzzles!</p>
{button}
<p style="color:#4a5563;">&#9203; {stats}</p>
<p style="font-size:14px;color:#4a5563;">No account needed. You can come back to the same link to edit your photos until the game starts.</p>"#,
        greet = esc(&greeting(e.participant_name)),
        org = esc(e.organizer),
        game = esc(e.game_name),
        message = e.message.map(quote).unwrap_or_default(),
        photos = photos,
        deadline = deadline_html,
        button = button(e.url, "Upload my photos"),
        stats = esc(&stats),
    );
    let text = format!(
        "{greet}\n\n{org} is preparing a TimeGuessr game: {game}. Everyone shares photos of themselves, then teams guess when and where each one was taken.\n\n{message}Please share {photos} of yourself, with the year and the exact place it was taken{deadline}.\n\nUpload your photos: {url}\n\n{stats}\n\nNo account needed. This link is personal: please don't forward it.",
        greet = greeting(e.participant_name),
        org = e.organizer,
        game = e.game_name,
        message = e.message.map(|m| format!("Message from {}:\n{}\n\n", e.organizer, m)).unwrap_or_default(),
        photos = photos,
        deadline = deadline_text,
        url = e.url,
        stats = stats,
    );
    Rendered {
        subject: format!("{} invited you to “{}”: share {}", e.organizer, e.game_name, photos),
        html: layout(&stats, &body, &footer_for(e.organizer, e.game_name)),
        text,
    }
}

pub fn reminder(e: &CollectionEmail) -> Rendered {
    let photos = plural(e.photos_required as i64, "photo", "photos");
    let stats = pending_sentence(e.pending, e.total);
    let progress = format!(
        "You've shared {} of {} so far.",
        e.photos_uploaded,
        plural(e.photos_required as i64, "photo", "photos")
    );
    let deadline_html = e
        .deadline
        .map(|d| format!("<p>The deadline is <strong>{}</strong>.</p>", esc(d)))
        .unwrap_or_default();
    let body = format!(
        r#"<p>{greet}</p>
<p>Friendly reminder: <strong>{org}</strong> is waiting for your photos for <strong>{game}</strong>. {progress}</p>
{deadline}
{button}
<p style="color:#4a5563;">&#9203; {stats}</p>
<p style="font-size:14px;color:#4a5563;">Don't want to share more photos? Open the link and click “I'm done” — you won't get further reminders.</p>"#,
        greet = esc(&greeting(e.participant_name)),
        org = esc(e.organizer),
        game = esc(e.game_name),
        progress = esc(&progress),
        deadline = deadline_html,
        button = button(e.url, "Upload my photos"),
        stats = esc(&stats),
    );
    let text = format!(
        "{greet}\n\nFriendly reminder: {org} is waiting for your photos for {game}. {progress}\n{deadline}\nUpload your photos: {url}\n\n{stats}\n\nDon't want to share more? Open the link and click \"I'm done\".",
        greet = greeting(e.participant_name),
        org = e.organizer,
        game = e.game_name,
        progress = progress,
        deadline = e.deadline.map(|d| format!("The deadline is {d}.\n")).unwrap_or_default(),
        url = e.url,
        stats = stats,
    );
    Rendered {
        subject: format!("Reminder: {} for “{}”", photos, e.game_name),
        html: layout(&stats, &body, &footer_for(e.organizer, e.game_name)),
        text,
    }
}

pub fn play_link(game_name: &str, organizer: &str, participant_name: Option<&str>, url: &str) -> Rendered {
    let body = format!(
        r#"<p>{greet}</p>
<p>The photos are in: <strong>{game}</strong> is ready to play! &#127918;</p>
<p>Gather your team, pick a team name, and guess when and where each photo was taken. Up to 500 points for the year and 500 for the place, for every photo.</p>
{button}"#,
        greet = esc(&greeting(participant_name)),
        game = esc(game_name),
        button = button(url, "Play now"),
    );
    Rendered {
        subject: format!("“{game_name}” is ready to play!"),
        html: layout("The game is ready", &body, &format!("Sent by {} via TimeGuessr.", esc(organizer))),
        text: format!(
            "{}\n\n{game_name} is ready to play! Gather your team, pick a team name and guess when and where each photo was taken.\n\nPlay: {url}",
            greeting(participant_name)
        ),
    }
}

pub fn notification(title: &str, body: &str, url: &str) -> Rendered {
    let html_body = format!(
        "<p style=\"font-size:18px;font-weight:600;\">{}</p><p style=\"white-space:pre-line;\">{}</p>{}",
        esc(title),
        esc(body),
        button(url, "Open TimeGuessr")
    );
    Rendered {
        subject: format!("[TimeGuessr] {title}"),
        html: layout(title, &html_body, "You can change your email notification settings from your TimeGuessr dashboard."),
        text: format!("{title}\n\n{body}\n\n{url}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_sentences() {
        assert_eq!(pending_sentence(1, 3), "1 of 3 participants hasn't finished uploading yet.");
        assert_eq!(pending_sentence(2, 3), "2 of 3 participants haven't finished uploading yet.");
        assert_eq!(pending_sentence(0, 3), "All 3 participants have finished uploading.");
    }

    #[test]
    fn escapes_user_content() {
        let r = invitation(&CollectionEmail {
            game_name: "<script>x</script>",
            organizer: "Eve & co",
            participant_name: None,
            message: Some("<img src=x onerror=alert(1)>"),
            photos_required: 2,
            photos_uploaded: 0,
            deadline: Some("Friday"),
            pending: 3,
            total: 3,
            url: "https://example.com/c/abc",
        });
        assert!(!r.html.contains("<script>"));
        assert!(!r.html.contains("<img"));
        assert!(r.html.contains("Eve &amp; co"));
        assert!(r.text.contains("3 of 3 participants"));
    }
}
