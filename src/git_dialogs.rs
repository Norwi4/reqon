use crate::{
    app::App,
    git::{Action, State as Repository},
    git_ops::BranchAction as B,
    git_ui::{Dialog, State},
    widgets as w,
};
use eframe::egui::{self, Ui};

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}
fn directory(ui: &mut Ui, value: &mut String) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(value).desired_width((ui.available_width() - 75.).max(100.)),
        );
        if ui.button("Browse…").clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                *value = path.to_string_lossy().into_owned();
            }
        }
    });
}
pub fn show(
    app: &mut App,
    ctx: &egui::Context,
    _cid: &str,
    repo: &Repository,
    form: &mut State,
    action: &mut Option<Action>,
) {
    let t = app.theme;
    if let Some((message, pending)) = form.confirm.clone() {
        let (accept, close) = w::modal(ctx, "Confirm Git action", 480., t, |ui| {
            ui.label(message);
            ui.add_space(10.);
            ui.add_enabled(!repo.busy, egui::Button::new("Confirm"))
                .clicked()
        });
        if accept {
            *action = Some(pending);
            form.confirm = None;
        } else if close {
            form.confirm = None;
        }
        return;
    }
    if form.clone_open {
        let (submit, close) = w::modal(ctx, "Clone repository", 540., t, |ui| {
            field(ui, "Repository URL", &mut form.clone_url);
            ui.label("Destination directory");
            directory(ui, &mut form.clone_path);
            ui.label(egui::RichText::new("Choose an empty directory. Credentials use your Git credential helper or the connected provider.").size(12.).color(t.muted));
            ui.add_enabled(
                app.git_clone.is_none()
                    && !form.clone_url.trim().is_empty()
                    && !form.clone_path.trim().is_empty(),
                egui::Button::new("Clone"),
            )
            .clicked()
        });
        if submit {
            app.clone_git(
                form.clone_url.trim().into(),
                std::path::PathBuf::from(form.clone_path.trim()),
            );
            form.clone_open = false;
        } else if close {
            form.clone_open = false;
        }
        return;
    }
    let Some(dialog) = form.dialog.clone() else {
        return;
    };
    let title = match &dialog {
        Dialog::Settings => "Git settings",
        Dialog::Remotes => "Git remotes",
        Dialog::Stashes => "Stashes",
        Dialog::NewBranch(_) => "New branch",
        Dialog::Rename(_) => "Rename branch",
        Dialog::Worktree(_) => "New worktree",
        Dialog::Push(_) => "Push commits",
    };
    let (done, close) = w::modal(ctx, title, 550., t, |ui| {
        let mut done = false;
        ui.add_enabled_ui(!repo.busy,|ui|match &dialog {
            Dialog::Settings=>{
                ui.label("Commit author for this repository");
                field(ui,"Name",&mut form.name);field(ui,"Email",&mut form.email);
                if ui.add_enabled(!form.name.trim().is_empty()&&!form.email.trim().is_empty(),egui::Button::new("Save")).clicked(){*action=Some(Action::Identity(form.name.trim().into(),form.email.trim().into()));done=true;}
                ui.separator();
                if let Some(root)=&repo.root {ui.label(egui::RichText::new(root.display().to_string()).size(12.).color(t.muted));}
                if ui.button("Manage remotes…").clicked(){form.dialog=Some(Dialog::Remotes);}
            }
            Dialog::Remotes=>{
                for (name,url) in &repo.remotes {
                    ui.horizontal(|ui|{
                        if ui.selectable_label(form.remote_name==*name,name).clicked(){form.remote_name=name.clone();form.remote_url=url.clone();}
                        ui.add(egui::Label::new(egui::RichText::new(url).size(12.).color(t.muted)).truncate());
                        if ui.small_button("Remove").clicked(){form.confirm=Some((format!("Remove remote '{name}' from this repository? The remote repository itself is preserved."),Action::RemoveRemote(name.clone())));}
                    });
                }
                ui.separator();field(ui,"Name",&mut form.remote_name);field(ui,"URL",&mut form.remote_url);
                if ui.add_enabled(!form.remote_name.trim().is_empty()&&!form.remote_url.trim().is_empty(),egui::Button::new("Save remote")).clicked(){*action=Some(Action::AddRemote(form.remote_name.trim().into(),form.remote_url.trim().into()));form.remote_url.clear();}
            }
            Dialog::Stashes=>{
                field(ui,"Stash message",&mut form.stash);
                if ui.add_enabled(!repo.files.is_empty(),egui::Button::new("Stash changes (including new files)")).clicked(){*action=Some(Action::Stash(form.stash.clone()));form.stash.clear();}
                ui.separator();
                if repo.stashes.is_empty(){ui.label("No stashed changes");}
                for (name,summary) in &repo.stashes {ui.push_id(name,|ui|{
                    ui.label(egui::RichText::new(format!("{name}  {summary}")).size(12.));
                    ui.horizontal(|ui|{
                        if ui.button("Apply").clicked(){*action=Some(Action::ApplyStash(name.clone(),false));done=true;}
                        if ui.button("Pop").clicked(){*action=Some(Action::ApplyStash(name.clone(),true));done=true;}
                        if ui.button("Drop").clicked(){form.confirm=Some((format!("Permanently drop {name}: {summary}?"),Action::DropStash(name.clone())));}
                    });
                });}
            }
            Dialog::NewBranch(base)=>{
                ui.label(format!("Create and check out a branch from {base}"));field(ui,"Branch name",&mut form.branch);
                if ui.add_enabled(!form.branch.trim().is_empty(),egui::Button::new("Create")).clicked(){*action=Some(Action::Branch(B::Create{name:form.branch.trim().into(),base:base.clone()}));done=true;}
            }
            Dialog::Rename(old)=>{
                ui.label(format!("Rename '{old}'"));field(ui,"New name",&mut form.branch);
                if ui.add_enabled(!form.branch.trim().is_empty()&&form.branch.trim()!=old,egui::Button::new("Rename")).clicked(){*action=Some(Action::Branch(B::Rename{old:old.clone(),new:form.branch.trim().into()}));done=true;}
            }
            Dialog::Worktree(base)=>{
                ui.label(format!("Create a separate working directory from {base}"));field(ui,"New branch",&mut form.branch);
                ui.label("Destination");directory(ui,&mut form.path);
                if ui.add_enabled(!form.branch.trim().is_empty()&&!form.path.trim().is_empty(),egui::Button::new("Create worktree")).clicked(){*action=Some(Action::Branch(B::Worktree{base:base.clone(),branch:form.branch.trim().into(),destination:form.path.trim().into()}));done=true;}
            }
            Dialog::Push(branch)=>{
                ui.label(format!("Local branch: {branch}"));
                if repo.remotes.is_empty(){ui.label("Add a remote before pushing commits.");if ui.button("Add remote…").clicked(){form.dialog=Some(Dialog::Remotes);}}
                else {
                    if !repo.remotes.iter().any(|(n,_)|n==&form.remote_name){form.remote_name=repo.push_remote.clone();}
                    egui::ComboBox::from_id_salt("push-remote").selected_text(&form.remote_name).show_ui(ui,|ui|{for(name,url)in &repo.remotes{ui.selectable_value(&mut form.remote_name,name.clone(),format!("{name} — {url}"));}});
                    ui.label(egui::RichText::new("Pushes to the tracked branch, or creates a branch with the same name and sets its upstream.").size(12.).color(t.muted));
                    if ui.button("Push").clicked(){*action=Some(Action::Branch(B::Push{branch:branch.clone(),remote:form.remote_name.clone()}));done=true;}
                }
            }
        });
        done
    });
    if done || close {
        form.dialog = None;
    }
}
