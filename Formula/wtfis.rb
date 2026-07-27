class Wtfis < Formula
  desc "Find projects fast from your terminal"
  homepage "https://github.com/prophesourvolodymyr/WTFIS-CLI"
  url "https://github.com/prophesourvolodymyr/WTFIS-CLI/archive/refs/tags/v1.0.3.tar.gz"
  version "1.0.3"
  license "WTFPL"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "Cargo.toml")
    bin.install_symlink "wtfis" => "cdd"
    pkgshare.install "shell/wtfis.zsh", "shell/wtfis.bash", "shell/wtfis.ps1"
  end

  def caveats
    <<~EOS
      WTFIS needs shell integration to change your parent shell directory.

      Zsh:
        echo 'source "$(brew --prefix)/share/wtfis/wtfis.zsh"' >> ~/.zshrc
        source ~/.zshrc

      Bash:
        echo 'source "$(brew --prefix)/share/wtfis/wtfis.bash"' >> ~/.bashrc
        source ~/.bashrc
    EOS
  end

  test do
    assert_match "wtfis", shell_output("#{bin}/wtfis --help 2>&1", 0)
  end
end
