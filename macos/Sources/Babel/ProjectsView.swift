// The sidebar: the translations, newest first, and the account's sync state.

import SwiftUI
import UniformTypeIdentifiers

struct ProjectsView: View {
    @Bindable var model: AppModel

    var body: some View {
        List(selection: $model.selection) {
            ForEach(model.projects) { p in
                ProjectRow(project: p)
                    .tag(p.id)
                    .contextMenu {
                        Button("Export .docx…") { model.export(p.id) }
                        Divider()
                        Button("Delete…", role: .destructive) { model.delete(p) }
                    }
            }
        }
        .listStyle(.sidebar)
        .overlay {
            if model.projects.isEmpty {
                ContentUnavailableView {
                    Label("No Translations Yet", systemImage: "doc.text")
                } description: {
                    Text("Open a Word document to start translating it side by side. The layout stays put; only the words change.")
                } actions: {
                    Button("Open Document…") { model.openDocument() }
                }
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            SyncBar(model: model)
        }
        .onDrop(of: [.fileURL], isTargeted: nil) { providers in
            for provider in providers {
                _ = provider.loadObject(ofClass: URL.self) { url, _ in
                    guard let url, url.pathExtension.lowercased() == "docx" else { return }
                    DispatchQueue.main.async { model.openDocument(url) }
                }
            }
            return true
        }
        .onDeleteCommand {
            if let id = model.selection, let p = model.projects.first(where: { $0.id == id }) { model.delete(p) }
        }
    }
}

struct ProjectRow: View {
    let project: Project

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(project.name)
                .fontWeight(.medium)
                .lineLimit(1)
                .truncationMode(.middle)
            Text("→ \(project.lang) · \(project.done) % · \(Fmt.ago(project.updated))")
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            ProgressView(value: Double(project.done), total: 100)
                .progressViewStyle(.linear)
                .controlSize(.mini)
        }
        .padding(.vertical, 3)
        .help(project.name)
    }
}

/// The account at the foot of the sidebar: how syncing goes, Sync Now; or Sign In.
struct SyncBar: View {
    @Bindable var model: AppModel

    var body: some View {
        if model.signedIn || !model.syncStatus.isEmpty {
            status
        } else {
            HStack {
                Button { Sheets.account() } label: { Label("Sign In to Sync…", systemImage: "icloud") }
                    .buttonStyle(.borderless)
                    .foregroundStyle(.secondary)
                    .help("Sync your translations with a Babel server")
                Spacer()
            }
            .font(.caption)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
    }

    private var status: some View {
        HStack(spacing: 6) {
            if model.syncing {
                ProgressView().controlSize(.mini)
            } else {
                Image(systemName: model.syncStatus.hasPrefix("Not synced") || model.syncStatus.hasPrefix("Signed out") ? "exclamationmark.icloud" : "checkmark.icloud")
                    .foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 0) {
                if model.signedIn {
                    Text("\(model.syncUser) on \(model.syncServer)").lineLimit(1).truncationMode(.middle)
                }
                if !model.syncStatus.isEmpty {
                    Text(model.syncStatus).foregroundStyle(.secondary).lineLimit(2)
                }
            }
            .font(.caption)
            .contentShape(Rectangle())
            .onTapGesture { Sheets.account() }
            .help("Account and sync")
            Spacer(minLength: 0)
            if model.signedIn {
                Button { model.syncNow() } label: { Image(systemName: "arrow.triangle.2.circlepath") }
                    .buttonStyle(.borderless)
                    .disabled(model.syncing)
                    .help("Sync Now")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }
}
