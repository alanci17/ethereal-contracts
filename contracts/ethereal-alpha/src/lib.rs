use scrypto::prelude::*;
use scrypto::blueprints::clock::TimePrecision;

type Addr = Result<ComponentAddress, (PackageAddress, String)>;

#[derive(ScryptoSbor, PartialEq, Clone)]
pub enum EDaoProposal {
  UpdateBranch(PackageAddress, String, String),
  UpdateSelf(PackageAddress, String, String)
}

#[derive(ScryptoSbor, PartialEq, Clone)]
pub enum DeltaProposal {
  // NOTE: doesn't work with NFTs
  Spend(ResourceAddress, Decimal, Addr, String),
  Issue(Decimal),
  Whitelist(ResourceAddress),

  // Omega
  OmegaVoteEDao(bool, u64),

  // EDao actions
  EDaoAddProposal(EDaoProposal),
  EDaoVote(bool, u64)
}


#[derive(ScryptoSbor, Clone)]
pub enum Action {
  // Protocol (i.e. EtherealUSD actions)
  ProtocolUpdateParams(), // TODO
  ProtocolUpdate(), // TODO

  // EDAO actions
  EDaoAddProposal(EDaoProposal),
  EDaoVote(bool, u64), 

  // Alpha actions 
  AlphaChangeParams(u64, Option<Decimal>, Decimal),

  // Delta actions 
  DeltaPuppeteer(DeltaProposal),
  DeltaAllowSpend(ResourceAddress, Decimal)
}

// EXTERNAL STATIC MODELS

external_component! {
  Dao {
    fn get_branch_addrs(&self) -> (ComponentAddress, ComponentAddress, ComponentAddress);
    fn add_proposal(&mut self, proposal: EDaoProposal, proof: Proof);
    fn vote(&mut self, vote: bool, proposal: u64, proof: Proof);
  }
}

external_component! {
  Delta {
    fn deposit(&mut self, input: Bucket);
    fn puppeteer(&mut self, proposal: DeltaProposal);
    fn allow_spend(&mut self, resource: ResourceAddress, amnt: Decimal);
  }
}

external_component! {
  Omega {
    fn get_issued(&self) -> Decimal;
  }
}


// Something 
#[derive(ScryptoSbor, Clone)]
pub enum Proposal {
  // no-effect poll-only
  // can have off-chain effects due to decisions
  TextOnly(String),
  // regular vote
  ActionSequence(Vec<Action>),
}

#[derive(ScryptoSbor)]
pub enum Vote {
  For(Decimal),
  Against(Decimal),
  Abstain(Decimal)
}

#[blueprint]
mod alpha {
  struct Alpha {
    dao_addr: ComponentAddress,
    power_zero: ResourceAddress,
    // authority of alpha
    // over protocol and itself
    power_alpha: Vault,
    // checked by omega
    // over proposal veto and vote power
    power_omega: ResourceAddress,

    // active proposals
    proposals: Vec<(Proposal, Instant, (Decimal, Decimal, Decimal))>,
    proposal_index: u64, // current top index

    gov_token: ResourceAddress, // $REAL

    // alpha parameters

    alpha_vote_duration: u64, // duration of votes in days before allowed to close 
    // TODO: Q: quorum vs pass threshold (different for differnt actions?)
    alpha_vote_quorum: Option<Decimal>, // minimum % participation before considered quorate
    alpha_proposal_payment: Decimal // in gov token, to submit proposal

  }

  impl Alpha {
    // instantiates the Alpha component
    pub fn from_nothing(
      dao_addr: ComponentAddress,
      power_zero: ResourceAddress,
      power_alpha: Bucket,
      power_omega: ResourceAddress,

      gov_token: ResourceAddress,

      alpha_vote_duration: u64, // 3 days
      alpha_vote_quorum: Option<Decimal>,
      alpha_proposal_payment: Decimal,

    ) -> ComponentAddress {
      let acc_rules = 
        AccessRulesConfig::new()
          .method("to_nothing", rule!(require(power_zero)), LOCKED)
          // hope dyslexia isn't a problem 
          .method("veto", rule!(require(power_omega)), LOCKED)
          .method("vote", rule!(require(power_omega)), LOCKED)
          .default(rule!(allow_all), LOCKED);

      Self {
        dao_addr,
        power_zero,
        power_alpha: Vault::with_bucket(power_alpha),
        power_omega,

        proposals: vec![],
        proposal_index: 1u64,

        gov_token,

        alpha_vote_duration,
        alpha_vote_quorum,
        alpha_proposal_payment
      }
      .instantiate()
      .globalize_with_access_rules(acc_rules)
    }

    // AuthRule: Power 0
    pub fn to_nothing(&mut self) -> Bucket {
      self.power_alpha.take_all()
      // TODO block calls if the soul was ripped out
    }

    // omega needs this
    pub fn get_proposal_indices(&self) -> (u64, u64) {
      (self.proposal_index, self.proposal_index + self.proposals.len() as u64)
    }

    // adds proposal to internal list of vote-able proposals
    pub fn add_proposal(&mut self, payment: Bucket, proposal: Proposal) {
      assert!( 
        payment.resource_address() == self.gov_token &&
        payment.amount() >= self.alpha_proposal_payment, 
        "incorrect payment for adding proposal");
      Delta::at(Dao::at(self.dao_addr).get_branch_addrs().1).deposit(payment);

      self.proposals.push(
        (proposal, 
        Clock::current_time_rounded_to_minutes(),
        (dec!(0), dec!(0), dec!(0))));
    }

    // AuthRule: power_omega
    // this call is trusted, alpha only aggregates the calls
    pub fn vote(&mut self, vote: Vote, proposal: u64) {
      assert!( proposal >= self.proposal_index, "veto on finalized proposal");
      let ix = proposal - self.proposal_index;

      let p = &self.proposals[ix as usize];
      assert!( p.2.0 >= dec!(0), "proposal veto'd");

      assert!(
        Clock::current_time_is_strictly_before( 
          p.1.add_days(self.alpha_vote_duration as i64).expect("failed to add days"), 
          TimePrecision::Minute ),
        "vote after closed" );

      let new = match vote {
        Vote::For(x) if x > dec!(0) => (p.2.0 + x, p.2.1, p.2.2),
        Vote::Against(x) if x > dec!(0) => (p.2.0, p.2.1 + x, p.2.2),
        Vote::Abstain(x) if x > dec!(0) => (p.2.0, p.2.1, p.2.2 + x),
        _ => panic!("nonpositive vote")
      };

      self.proposals[ix as usize] = (p.0.clone(), p.1, new);
    }
    
    // AuthRule: power_omega
    pub fn veto(&mut self, proposal: u64) {
      assert!( proposal >= self.proposal_index, "veto on finalized proposal");
      let ix = proposal - self.proposal_index;

      let p = &self.proposals[ix as usize];
      
      assert!(
        Clock::current_time_is_strictly_before( 
          p.1.add_days(self.alpha_vote_duration as i64).expect("failed to add days"), 
          TimePrecision::Minute ),
        "veto after closed" );
      
      // veto state
      let new = (dec!(-1), dec!(0), dec!(0));
      
      self.proposals[ix as usize] = (p.0.clone(), p.1, new);
    }

    // either executes a proposal or not, depending on result
    // and then removes it out of the internal list 
    pub fn finalize_proposal(&mut self) {
      fn is_quorate(daoa: &ComponentAddress, qu: &Option<Decimal>, v: Decimal) -> bool {
        if let Some(q) = *qu {
          v / Omega::at(Dao::at(*daoa).get_branch_addrs().2).get_issued() > q
        } else {
          true
        }
      } 
      let p = self.proposals[0].clone(); // fails if empty
      
      assert!(
        Clock::current_time_is_strictly_after( 
          p.1.add_days(self.alpha_vote_duration as i64).expect("failed to add days"), 
          TimePrecision::Minute ),
        "finalize before closed" );

      match p.2 {
        (y,n,a) if y > n && is_quorate(&self.dao_addr, &self.alpha_vote_quorum, y+n+a) 
          => self._execute_proposal(&p.0),
        _ => info!("proposal rejected")
      };

      // note: in future might want to add custom thresholds
      // for different actions i.e. higher for more important
      // initially everything is a majority win
      
      self.proposals.remove(0);
      self.proposal_index += 1;
    }

    // PRIVATE FUNCTIONS 

    // raw proposal execute logic
    // it better fucking grab the Component/Package into the fucking scope
    fn _execute_proposal(&mut self, prop: &Proposal) {
      match prop {
        Proposal::TextOnly(_) => (),
        Proposal::ActionSequence(v) => {
          for action in v {
            self._execute_single_action(action);
          }
        }
      }
    }

    // eval
    fn _execute_single_action(&mut self, action: &Action) {
      match action {
        // Protocol actions
        Action::ProtocolUpdateParams() => (), // TODO
        Action::ProtocolUpdate() => (), // TODO

        // EDAO actions
        Action::EDaoAddProposal(edao_proposal) => {
          let proof = self.power_alpha.create_proof();
          Dao::at(self.dao_addr).add_proposal(edao_proposal.clone(), proof)
        },
        Action::EDaoVote(vote, proposal) => {
          let proof = self.power_alpha.create_proof();
          Dao::at(self.dao_addr).vote(*vote, *proposal, proof)
        }, 

        // Alpha actions 
        Action::AlphaChangeParams(vote_duration, vote_quorum, proposal_payment) => {
          self.alpha_vote_duration = *vote_duration;
          self.alpha_vote_quorum = *vote_quorum;
          self.alpha_proposal_payment = *proposal_payment;
        },

        // Delta actions 
        Action::DeltaPuppeteer(delta_proposal) => 
          self.power_alpha.authorize(||
            Delta::at(
              Dao::at(self.dao_addr).get_branch_addrs().1
            ).puppeteer(delta_proposal.clone())
        ),
        Action::DeltaAllowSpend(asset, amount) => 
          self.power_alpha.authorize(|| 
            Delta::at(
              Dao::at(self.dao_addr).get_branch_addrs().1
            ).allow_spend(*asset, *amount)
        )
      }
    }
  }
}