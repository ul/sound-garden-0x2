# frozen_string_literal: true
#
# Asciidoctor extension for the Sound Garden book.
#
# 1. `sg` source highlighting, with the op reference (audio_program/src/help.adoc) as tooltips.
# 2. `[sound]` listing blocks: the program is shown, rendered by book_render into audio and
#    figures (cached by content hash), and exported as a plain file readers can play.
#
#   .Tuning fork
#   [sound, seconds=3, show="wave spectrum", from=0, to=0.01]
#   ----
#   440 s <1>
#   0.2 * <2>
#   ----
#
# Callout markers are stripped before rendering and exporting, so programs stay valid.

require 'asciidoctor'
require 'asciidoctor/extensions'
require 'asciidoctor/syntax_highlighter/rouge'
require 'cgi'
require 'digest'
require 'fileutils'
require 'json'
require 'open3'

# Sources, help.adoc and figures are UTF-8 whatever the locale says.
Encoding.default_external = Encoding::UTF_8

module SoundGarden
  ROOT = File.expand_path '../..', __dir__
  HELP = File.join ROOT, 'audio_program/src/help.adoc'
  RENDERER = File.join ROOT, 'target/release/book_render'
  STACK_OPS = %w[pop dup swap rot].freeze
  CALLOUTS_RX = /(?:\s*\\?<!?(--|)(?:\d+|\.)\1>)+\s*$/

  # Op name => reference entry, and parametric prefixes like "pat:" => reference entry.
  # An entry is { name:, sig:, text:, group: }, shown by the popover in theme/docinfo-footer.html.
  module Help
    module_function

    def index
      @index ||= begin
        exact = {}
        prefixes = {}
        group = nil
        File.foreach(HELP, chomp: true) do |line|
          if (m = line.match(/\A=== (.+)/))
            group = m[1].strip
          elsif (m = line.match(/\A(\S.*?)::\s+(.*)\z/))
            names, desc = m[1], m[2].strip
            sig = desc[/\A\([^)]*\)\s*->/]
            text = sig ? desc.delete_prefix(sig).strip : desc
            text = "#{text[0, 320]}…" if text.length > 320
            entry = { name: names, sig: sig&.sub(/\s*->\z/, ' →'), text: text, group: group }
            names.split(', ').each do |name|
              if (i = name.index(':<'))
                prefixes[name[0..i]] ||= entry
              else
                exact[name] ||= entry
              end
            end
          end
        end
        [exact, prefixes]
      end
    end

    def lookup token
      exact, prefixes = index
      return [exact[token], nil] if exact.key? token
      # A bare parametric word using its defaults, e.g. "limit" for "limit:<R>".
      return [prefixes["#{token}:"], nil] if prefixes.key? "#{token}:"
      # Longest matching "name:" prefix, e.g. "wt:src:4" => "wt:".
      if (i = token.index(':')) && i > 0 && (entry = prefixes[token[0..i]])
        return [entry, i + 1]
      end
      [nil, nil]
    end
  end

  # Tokenizes a program into HTML spans. Whitespace (and so the author's layout) is preserved.
  module Highlight
    module_function

    def html source
      tokens = source.scan(/\s+|\S+/)
      words = tokens.each_index.reject { |i| tokens[i].match?(/\A\s/) }
      comment = comment_ranges(tokens, words)
      templates = tokens.filter_map { |t| t[/\A(?:def)?:([^\s:]+)\z/, 1] }
      tokens.each_with_index.map do |tok, i|
        next tok if tok.match?(/\A\s/)
        next span('c', tok) if comment[i]
        word(tok, templates)
      end.join
    end

    # Marks comments: `( ... )` groups, which run until their parentheses balance, and
    # `[ ... ] drop` or `[ ... ] --` spans (with nesting).
    def comment_ranges tokens, words
      marks = {}
      stack = []
      depth = 0
      words.each_with_index do |ti, wi|
        tok = tokens[ti]
        if depth > 0 || tok.start_with?('(')
          depth = [depth + tok.count('(') - tok.count(')'), 0].max
          marks[ti] = true
          next
        end
        case tok
        when '[' then stack << wi
        when ']'
          next unless (open = stack.pop)
          nxt = words[wi + 1]
          (words[open]..nxt).each { |k| marks[k] = true } if nxt && %w[drop --].include?(tokens[nxt])
        end
      end
      marks
    end

    def word tok, templates
      case tok
      when /\A-?(?:\d+\.?\d*|\.\d+)(?:e-?\d+)?(?:\/-?(?:\d+\.?\d*|\.\d+))?\z/
        span('num', tok)
      when /\A[a-g][#b]?-?\d\z/
        span('note', tok, 'pitch as frequency in Hz (a4 = 440)')
      when /\A[A-G][#b]?-?\d\z/
        span('note', tok, 'pitch as MIDI note number (A4 = 69, C4 = 60)')
      when /\A([<>=])(.+)\z/
        verb = { '<' => 'read variable', '>' => 'move top of stack into variable', '=' => 'copy top of stack into variable' }[$1]
        span('var', tok, "#{verb} #{$2}")
      when '[', ']'
        span('q', tok, 'compile-time quotation')
      when /\A(?:def)?:([^\s:]+)\z/
        span('def', tok, "define template #{$1} from the preceding quotation")
      else
        tip, split = Help.lookup(tok)
        if tip && split
          span('op', tok[0...split], tip) + span('arg', tok[split..])
        elsif tip
          span(STACK_OPS.include?(tok) || tok.start_with?('dig:', 'bury:') ? 'stk' : 'op', tok, tip)
        elsif templates.include?(tok)
          span('tpl', tok, "template #{tok}")
        else
          span('unk', tok, 'unknown word: ignored by the compiler')
        end
      end
    end

    # tip is a Help entry or a plain explanation.
    def span cls, text, tip = nil
      tip = { text: tip } if tip.is_a? String
      data = (tip || {}).filter_map do |k, v|
        %( data-#{k == :text ? 'tip' : k}="#{CGI.escapeHTML v}") if v
      end.join
      %(<span class="sg-#{cls}"#{data}>#{CGI.escapeHTML text}</span>)
    end
  end

  # Rouge for everything, our own highlighter for `sg`. Token colours live in the book theme.
  class Highlighter < Asciidoctor::SyntaxHighlighter::RougeAdapter
    register_for 'sg-rouge'

    def highlight node, source, lang, opts
      return super unless lang == 'sg'
      html = Highlight.html source
      opts[:number_lines] && opts[:callouts] ? [html, nil] : html
    end

    def docinfo? _location
      false
    end

    def write_stylesheet? _doc
      false
    end
  end

  # Parses a [sound] block: shows the program and renders its media. Numbering, the exported
  # file and the player are added by SoundNumbering, once chapter numerals exist.
  class SoundBlock < Asciidoctor::Extensions::BlockProcessor
    use_dsl
    named :sound
    on_context :listing
    parse_content_as :raw

    def process parent, reader, attrs
      doc = parent.document
      # The block reader has no position; the document reader has just passed the closing delimiter.
      where = { source_location: doc.reader.cursor_at_prev_line }
      source = reader.lines.join("\n")
      program = "#{reader.lines.map { |l| l.sub(CALLOUTS_RX, '') }.join("\n").rstrip}\n"
      outdir = doc.options[:to_dir] || doc.attr('outdir') || Dir.pwd

      box = create_block parent, :open, nil, { 'role' => 'soundblock' }
      box.id = attrs['id']
      listing = create_block box, :listing, source,
                             { 'style' => 'source', 'language' => 'sg', 'role' => 'program' }
      listing.style = 'source'
      Asciidoctor::Parser.catalog_callouts source, doc
      listing.commit_subs
      if attrs.key? 'fold-option'
        # %fold: the score is folded away under the player, for an excerpt that opens a chapter.
        score = create_block box, :example, nil, { 'role' => 'score' }
        score.title = 'Score'
        score.set_option 'collapsible'
        score << listing
        box << score
      else
        box << listing
      end
      box.instance_variable_set :@sound, {
        program: program, title: attrs['title'], attrs: attrs, outdir: outdir,
        media: Media.render(outdir, doc.base_dir, program, attrs, where, Asciidoctor::LoggerManager.logger),
      }
      box
    end
  end

  class SoundNumbering < Asciidoctor::Extensions::TreeProcessor
    def process doc
      caption = doc.attr 'sound-caption', 'Sound'
      counts = Hash.new(0)
      boxes = doc.find_by(context: :open) { |b| b.instance_variable_defined? :@sound }
      boxes.each do |box|
        sound = box.instance_variable_get :@sound
        chapter = chapter_of box
        number = counts[chapter] += 1
        label = [chapter&.numeral, number].compact.join('.')
        box.id ||= "sound-#{label.tr('.', '-')}"
        doc.register :refs, [box.id, box]
        box.set_attr 'reftext', "#{caption} #{label}"
        box.title = %(#{caption} #{label}#{sound[:title] ? ". #{sound[:title]}" : ''})
        example = export(sound[:outdir], chapter, number, sound[:title], sound[:program])
        box << create_pass_block(box, Media.player_html(sound[:media], example, sound[:attrs]), {})
        figures = Media.figures_html(sound[:media], sound[:attrs])
        box << create_pass_block(box, figures, {}) unless figures.empty?
      end
      nil
    end

    private

    def chapter_of node
      node = node.parent until node.nil? || (node.context == :section && node.level == 1)
      node
    end

    def slug text
      text.to_s.downcase.gsub(/<[^>]*>/, '').gsub(/[^a-z0-9]+/, '-').gsub(/\A-|-\z/, '')
    end

    # examples/03-filters/02-resonance.txt: what the reader plays with `play_program <`.
    def export outdir, chapter, number, title, program
      numeral = chapter&.numeral
      numeral = format('%02d', numeral.to_i) if numeral.to_s.match?(/\A\d+\z/)
      dir = chapter ? [numeral, slug(chapter.title)].compact.join('-') : 'front'
      name = [format('%02d', number), slug(title)].reject(&:empty?).join('-')
      rel = "examples/#{dir}/#{name}.txt"
      path = File.join(outdir, rel)
      FileUtils.mkdir_p File.dirname(path)
      File.write(path, program) unless File.exist?(path) && File.read(path) == program
      rel
    end
  end

  # Branches and exercise answers fold: [.branch] and [.answer] example blocks become
  # collapsible, so authors write the role and never the option.
  class FoldedBoxes < Asciidoctor::Extensions::TreeProcessor
    def process doc
      doc.find_by(context: :example) { |b| b.has_role?('branch') || b.has_role?('answer') }.each do |block|
        block.set_option 'collapsible'
        block.title = 'Answer' if block.has_role?('answer') && !block.title?
      end
      nil
    end
  end

  # Rendering through book_render, cached by content hash, and the HTML around the results.
  module Media
    module_function

    def render outdir, base_dir, program, attrs, where, logger
      show = attrs.fetch('show', '').split(/[\s,]+/)
      args = { 'seconds' => attrs.fetch('seconds', '4') }
      %w[fade from to fmax fscale].each { |k| args[k] = attrs[k] if attrs[k] }
      key = Digest::SHA256.hexdigest([program, args.sort, show.sort, SoundGarden.renderer_digest].inspect)[0, 16]
      base = File.join(outdir, 'media', key)
      files = {
        'audio' => "#{base}.mp3",
        'overview' => "#{base}.overview.svg",
        'wave' => "#{base}.wave.svg",
        'spectrum' => "#{base}.spectrum.svg",
        'spectrogram' => "#{base}.spectrogram.png",
        'stats' => "#{base}.json",
      }
      wanted = %w[audio overview stats] + (show & %w[wave spectrum spectrogram])
      SoundGarden.used_media.merge(wanted.map { |k| files[k] })

      unless wanted.all? { |k| File.exist? files[k] }
        FileUtils.mkdir_p File.dirname(base)
        cmd = [RENDERER, '-']
        args.each { |k, v| cmd.push "--#{k}", v.to_s }
        (wanted - ['stats']).each { |k| cmd.push "--#{k}", files[k] }
        out, err, status = Open3.capture3(*cmd, stdin_data: program, chdir: base_dir)
        unless status.success?
          warn_at logger, "book_render failed: #{err.strip}", where
          return { 'files' => {}, 'abs' => {}, 'stats' => {} }
        end
        File.write(files['stats'], out)
      end

      stats = JSON.parse(File.read(files['stats']))
      # Re-reported on cached builds too, so a warning can't hide behind the cache.
      stats['warnings'].each { |w| warn_at logger, "sound program: #{w}", where }
      warn_at logger, "sound program clips (peak #{stats['peak'].max})", where if stats['clipped'].any?(&:positive?)
      rel = ->(f) { f.delete_prefix("#{outdir}/") }
      { 'files' => wanted.to_h { |k| [k, rel[files[k]]] }, 'abs' => files, 'stats' => stats }
    end

    def warn_at logger, message, where
      loc = where[:source_location]
      logger.warn(loc ? "#{loc}: #{message}" : message)
    end

    def player_html media, example, attrs
      f = media['files']
      return '' unless f['audio']
      overview = File.read(media['abs']['overview'])
      loop = attrs.key?('loop-option') ? ' loop' : ''
      peak = media['stats']['peak']&.max
      title = "The program as a file, for play_program &lt; #{File.basename example}"
      title += format(' · peak %.2f', peak) if peak
      <<~HTML
        <div class="sound-player" data-state="paused">
          <button class="sound-play" type="button" aria-label="Play">
            <svg viewBox="0 0 16 16" aria-hidden="true"><path class="i-play" d="M4 2.5v11l9.5-5.5z"/><path class="i-pause" d="M3.5 2.5h3v11h-3zM9.5 2.5h3v11h-3z"/></svg>
          </button>
          <div class="sound-scrub">#{overview}<div class="sound-progress"></div></div>
          <span class="sound-time">#{format('%.1f', attrs.fetch('seconds', '4').to_f)} s</span>
          <a class="sound-file" href="#{example}" title="#{title}">.txt</a>
          <audio preload="none" src="#{f['audio']}"#{loop}></audio>
        </div>
      HTML
    end

    # The render settings ride along as data-render, so live editing can redraw the figures of
    # an edited program the same way (theme/docinfo-footer.html, theme/figures-worker.js).
    def figures_html media, attrs
      f = media['files']
      abs = media['abs']
      settings = {
        seconds: attrs.fetch('seconds', '4').to_f,
        fade: attrs['fade']&.to_f,
        show: attrs.fetch('show', '').split(/[\s,]+/),
        from: attrs['from']&.to_f,
        to: attrs['to']&.to_f,
        fmax: attrs['fmax']&.to_f,
        fscale: attrs['fscale'],
      }.compact
      parts = []
      parts << %(<figure class="sound-figure wave">#{File.read abs['wave']}</figure>) if f['wave']
      parts << %(<figure class="sound-figure spectrum">#{File.read abs['spectrum']}</figure>) if f['spectrum']
      if f['spectrogram']
        parts << %(<figure class="sound-figure spectrogram"><img src="#{f['spectrogram']}" alt="Spectrogram" loading="lazy"></figure>)
      end
      parts.empty? ? '' : %(<div class="sound-figures" data-render="#{CGI.escapeHTML JSON.generate(settings)}">#{parts.join}</div>)
    end
  end

  # Callout marks become bare numbers styled as circles, the subtitle is shown, and media no longer referenced is
  # deleted so the build directory doesn't grow forever.
  class Finish < Asciidoctor::Extensions::Postprocessor
    def process doc, output
      output = output.gsub(%r{<b class="conum">\((\d+)\)</b>}, '<b class="conum">\\1</b>')
      # The HTML converter ignores :subtitle:, so add it under the title ourselves.
      if (subtitle = doc.attr 'subtitle')
        output = output.sub(%r{(<div id="header">\s*<h1>.*?)(</h1>)}m) { "#{$1}<span class=\"subtitle\">#{subtitle}</span>#{$2}" }
      end
      outdir = doc.options[:to_dir] || doc.attr('outdir')
      used = SoundGarden.used_media
      if outdir && !used.empty?
        Dir.glob(File.join(outdir, 'media', '*')).each { |f| File.delete f unless used.include? f }
      end
      output
    end
  end

  def self.used_media
    @used_media ||= Set.new
  end

  # Re-render everything when the engine changes, not only when a program does.
  def self.renderer_digest
    @renderer_digest ||= File.exist?(RENDERER) ? Digest::SHA256.file(RENDERER).hexdigest : 'missing'
  end
end

Asciidoctor::Extensions.register do
  block SoundGarden::SoundBlock
  tree_processor SoundGarden::SoundNumbering
  tree_processor SoundGarden::FoldedBoxes
  postprocessor SoundGarden::Finish
end
